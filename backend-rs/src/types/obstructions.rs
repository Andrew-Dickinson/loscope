use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io;
use async_fn_stream::fn_stream;
use futures_util::Stream;
use ndarray::{Array2, Axis};
use rocket::serde::{Deserialize, Serialize};
use tiff::decoder::{Decoder, DecodingResult};
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};
use crate::types::coords::NYSCoords2;
use crate::types::errors::AssetErr;
use crate::types::obj_writer::{RooftopObjWriter, MAX_OBJ_SIZE_USFT};
use crate::types::tiles::{TileId};
use crate::yield_str;

#[derive(Serialize,Deserialize,SchemaWrite,SchemaRead)]
pub enum ObstructionTypesFilter {
    All,
    Specific(Vec<String>),
}

impl Default for ObstructionTypesFilter {
    fn default() -> ObstructionTypesFilter {
        ObstructionTypesFilter::All
    }
}

#[derive(Debug,Serialize,Deserialize,Eq,Hash,PartialEq,Clone)]
pub struct ObstructionType(String);
pub type ObstructionId = Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AttributeValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

impl ObstructionType {
    pub fn parse(input_str: &str) -> Result<Self, ()> {
        // TODO: Convert to enum?
        Ok(ObstructionType(input_str.to_string()))
    }
}

impl Display for ObstructionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

// Deserialization helper that accepts both the current `offset_nys` format and the legacy
// `x_offset`/`y_offset` flat fields. Only used for deserialization; serialization is derived
// directly on ObstructionMeta and always emits `offset_nys`.
#[derive(Deserialize)]
struct ObstructionMetaDeHelper {
    #[serde(rename = "obstruction_id")]
    id: ObstructionId,
    #[serde(rename = "obstruction_type")]
    type_: ObstructionType,
    attributes: HashMap<String, AttributeValue>,
    #[serde(rename = "offset_nys")]
    sw_offset: Option<NYSCoords2>,
    x_offset: Option<f64>,
    y_offset: Option<f64>,
    tile_ids: Vec<TileId>,
}

impl TryFrom<ObstructionMetaDeHelper> for ObstructionMeta {
    type Error = String;

    fn try_from(h: ObstructionMetaDeHelper) -> Result<Self, Self::Error> {
        let sw_offset = match h.sw_offset {
            Some(coords) => coords,
            None => match (h.x_offset, h.y_offset) {
                (Some(x), Some(y)) => NYSCoords2::new(x, y),
                _ => return Err(
                    "missing offset: need either `offset_nys` or both `x_offset` and `y_offset`"
                        .to_string()
                ),
            },
        };
        Ok(ObstructionMeta { id: h.id, type_: h.type_, attributes: h.attributes, sw_offset, tile_ids: h.tile_ids })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(try_from = "ObstructionMetaDeHelper")]
pub struct ObstructionMeta {
    #[serde(rename = "obstruction_id")]
    id: ObstructionId,

    #[serde(flatten, rename = "obstruction_type")]
    type_: ObstructionType,

    attributes: HashMap<String, AttributeValue>,

    #[serde(rename = "offset_nys")]
    sw_offset: NYSCoords2,

    // Tiles intersected by the footprint
    tile_ids: Vec<TileId>
}

#[derive(Debug)]
pub struct ObstructionRaster {
    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add sw_offset to get global position)
    // Pixels outside the mask=true footprint are set to 0
    heightmap: Array2<u16>,
}

impl ObstructionRaster {
    pub fn read_from_tiff(obstruction_id: ObstructionId, file: File) -> Result<ObstructionRaster, AssetErr> {
        let io = std::io::BufReader::new(file);

        // We would love to use a try here to scope the ?s but it's only available in nightly,
        // so a closure it is
        let inner = move || -> Result<Array2::<u16>, Box<dyn std::error::Error>> {
            let mut reader = Decoder::new(io)?;
            let image_data = reader.read_image()?;
            let width: usize = reader.dimensions()?.0.try_into()?;
            let height: usize = reader.dimensions()?.1.try_into()?;

            let colortype = reader.colortype()?;
            if colortype != tiff::ColorType::Gray(16) {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData, format!("Invalid datatype: {:?}", colortype)
                )))
            }

            if let DecodingResult::U16(image_data) = image_data {
                Ok(Array2::from_shape_vec((height, width), image_data)
                    .or_else(|arr_err| Err(Box::new(arr_err)))?)
            } else {
                Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData, format!("Invalid datatype: {:?}", image_data)
                )))
            }
        };

        let image_data = inner()
            .or_else(|err| Err(
                AssetErr::AssetContentError(
                    format!("Error parsing obstruction tiff for id {obstruction_id}: {err}")
                )
            ))?;

        Ok(ObstructionRaster { heightmap: image_data })
    }

    pub fn to_obj_stream(&self, type_: ObstructionType, id: ObstructionId) -> impl Stream<Item = String> {
        let heightmap = &self.heightmap;

        let heightmap_ft = heightmap.map(|z_in| f64::from(*z_in) / 12.0);
        assert!(heightmap_ft.nrows() < MAX_OBJ_SIZE_USFT);
        assert!(heightmap_ft.ncols() < MAX_OBJ_SIZE_USFT);

        fn_stream(|e| async move {
            yield_str!(e, "# Obstruction heightmap terrain\n");
            e.emit(format!("# Obstruction id: {}\n", id)).await;
            e.emit( format!("# Obstruction type: {}\n", type_)).await;
            yield_str!(e, "# X = easting (local), Y = northing (local), Z = elevation (ft)\n");
            yield_str!(e, "o heightmap\n\n");

            let mut writer = RooftopObjWriter::new(&e);

            for (xi, col) in heightmap_ft.axis_iter(Axis(0)).into_iter().enumerate() {
                for (yi, z_ft) in col.iter().enumerate() {
                    if *z_ft == 0.0 { continue; }

                    // as f64 is safe per assertions above about
                    // max(xi, yi) = max(nrows, ncols) < MAX_OBJ_SIZE_USFT
                    let (x0, y0) = (xi as f64, yi as f64);
                    let (x1, y1) = (x0 + 1.0, y0 + 1.0);
                    writer.write_horizontal_face(x0, x1, y0, y1, *z_ft).await;

                    // Side faces
                    for (dxi, dyi, ax, ay, bx, by) in [
                        ( 0, -1, x0, y0, x1, y0),
                        ( 0,  1, x1, y1, x0, y1),
                        ( 1,  0, x1, y0, x1, y1),
                        (-1,  0, x0, y1, x0, y0),
                    ] {
                        let (delta_xi, delta_yi): (i8, i8) = (dxi, dyi);
                        let maybe_adj_z = xi.checked_add_signed(delta_xi.into())
                            .zip(yi.checked_add_signed(delta_yi.into()))
                            .and_then(|adj_xy| heightmap_ft.get([adj_xy.0, adj_xy.1]));

                        // Unlike the rooftop, we want to draw vertical faces for the sides of
                        // the obstruction, we fill in "gaps" from the sides of the obstruction
                        // with 0.0 so we draw the sides into the ground
                        let adj_z = maybe_adj_z.unwrap_or(&0.0);

                        // To avoid duplicate vertical faces, the top face "wins", and we don't draw
                        // the side if the adjacent pixel is below this one
                        if adj_z >= &*z_ft { continue }

                        writer.write_vertical_face(ax, bx, ay, by, *z_ft, *adj_z).await;
                    }
                }
            }
        })
    }
}