use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io;
use async_fn_stream::fn_stream;
use derive_getters::Getters;
use derive_new::new;
use futures_util::Stream;
use ndarray::Array2;
use rocket::serde::{Deserialize, Serialize};
use tiff::decoder::{Decoder, DecodingResult};
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};
use crate::types::coords::NYSCoords2;
use crate::types::errors::AssetErr;
use crate::types::obj_writer::{append_obj_row, MAX_OBJ_SIZE_USFT};
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

#[derive(Debug, Serialize, Deserialize, Getters, new)]
#[serde(try_from = "ObstructionMetaDeHelper")]
pub struct ObstructionMeta {
    #[serde(rename = "obstruction_id")]
    id: ObstructionId,

    #[serde(rename = "obstruction_type")]
    type_: ObstructionType,

    attributes: HashMap<String, AttributeValue>,

    #[serde(rename = "offset_nys")]
    sw_offset: NYSCoords2,

    // Tiles intersected by the footprint
    tile_ids: Vec<TileId>
}

impl ObstructionMeta {
    pub fn set_type(&mut self, new_type: ObstructionType){
        self.type_ = new_type;
    }
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
        let heightmap = self.heightmap.clone();
        assert!(heightmap.nrows() < MAX_OBJ_SIZE_USFT);
        assert!(heightmap.ncols() < MAX_OBJ_SIZE_USFT);

        fn_stream(|e| async move {
            e.emit(format!(
                "# Obstruction heightmap terrain\n\
                 # Obstruction id: {id}\n\
                 # Obstruction type: {type_}\n\
                 # X = easting (local), Y = northing (local), Z = elevation (ft)\n\
                 o heightmap\n\n"
            )).await;

            let mut vi: usize = 0;
            let mut buf = String::with_capacity(16 * 1024);

            for xi in 0..heightmap.nrows() {
                append_obj_row(
                    xi, &heightmap, &mut vi, &mut buf,
                    |_xi, _yi, z_in| z_in == 0,
                    // Draw side face down to adj_z whenever adj is strictly lower.
                    // OOB neighbors get adj_raw=0, which is always < z_in (pixel is non-zero),
                    // so the face is drawn to the ground — correct for building sides.
                    |_adj_idx, adj_raw, z_in| {
                        if adj_raw >= z_in { None } else { Some(adj_raw as f64 / 12.0) }
                    },
                );
                if buf.len() >= 16 * 1024 {
                    e.emit(std::mem::take(&mut buf)).await;
                }
            }

            if !buf.is_empty() { e.emit(buf).await; }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_PAYLOAD: &str = r#"{
        "obstruction_id": "83167e5c-c108-4d85-905c-6dc3224cc367",
        "obstruction_type": "new_construction_building_footprint",
        "attributes": {
            "bin": "2129799",
            "bbl": "2023440027",
            "ground_elevation": 34.0,
            "height_roof": 85.0,
            "geom_source": "Other (Manual)",
            "construction_year": 2025,
            "last_status_type": "Constructed"
        },
        "tile_ids": ["2235_21", "2235_22"],
        "x_offset": 1003728,
        "y_offset": 235953,
        "width": 155,
        "height": 229,
        "raster_file": "83167e5c-c108-4d85-905c-6dc3224cc367.tif"
    }"#;

    #[test]
    fn legacy_payload_deserializes_offset() {
        let meta: ObstructionMeta = serde_json::from_str(LEGACY_PAYLOAD).unwrap();
        assert_eq!(*meta.sw_offset.easting(), 1003728.0);
        assert_eq!(*meta.sw_offset.northing(), 235953.0);
    }

    #[test]
    fn legacy_payload_deserializes_attributes() {
        let meta: ObstructionMeta = serde_json::from_str(LEGACY_PAYLOAD).unwrap();
        assert_eq!(meta.attributes.len(), 7);

        assert!(matches!(meta.attributes.get("bin"), Some(AttributeValue::String(s)) if s == "2129799"));
        assert!(matches!(meta.attributes.get("ground_elevation"), Some(AttributeValue::Number(_))));
        assert!(matches!(meta.attributes.get("construction_year"), Some(AttributeValue::Number(_))));
    }

    #[test]
    fn legacy_payload_serializes_attributes_back_out() {
        let meta: ObstructionMeta = serde_json::from_str(LEGACY_PAYLOAD).unwrap();
        let json: serde_json::Value = serde_json::to_value(&meta).unwrap();

        let attrs = json.get("attributes").expect("attributes key missing");
        assert_eq!(attrs.get("bin").and_then(|v| v.as_str()), Some("2129799"));
        assert_eq!(attrs.get("ground_elevation").and_then(|v| v.as_f64()), Some(34.0));
        assert_eq!(attrs.get("construction_year").and_then(|v| v.as_f64()), Some(2025.0));
    }
}