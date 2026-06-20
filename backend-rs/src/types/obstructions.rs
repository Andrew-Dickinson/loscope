use crate::types::coords::NYSCoords2;
use crate::types::errors::AssetErr;
use crate::types::obj_writer::{MAX_OBJ_SIZE_USFT, append_obj_row};
use crate::types::tiles::TileId;
use async_fn_stream::fn_stream;
use derive_getters::Getters;
use derive_new::new;
use futures_util::Stream;
use ndarray::Array2;
use rocket::serde::de::{Error, SeqAccess, Unexpected, Visitor};
use rocket::serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::str::FromStr;
use std::{fmt, io};
use std::io::{Seek, Write};
use strum::ParseError;
use strum_macros::{AsRefStr, EnumString};
use tiff::TiffError;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::encoder::{TiffEncoder, colortype};
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead,Default)]
pub enum ObstructionTypesFilter {
    #[default]
    All,
    Specific(Vec<ObstructionType>),
}

impl ObstructionTypesFilter {
    pub fn includes(&self, type_: &ObstructionType) -> bool {
        match self {
            ObstructionTypesFilter::All => true,
            ObstructionTypesFilter::Specific(allowed_types) => allowed_types.contains(type_),
        }
    }
}

impl Serialize for ObstructionTypesFilter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ObstructionTypesFilter::All => serializer.serialize_str("*"),
            ObstructionTypesFilter::Specific(allowed_types) => allowed_types.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ObstructionTypesFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        pub struct ObstructionTypesFilterVisitor;
        impl<'de> Visitor<'de> for ObstructionTypesFilterVisitor {
            type Value = ObstructionTypesFilter;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "either a '*' literal or an array of strings")
            }
            fn visit_str<E>(self, input_str: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if input_str == "*" {
                    Ok(ObstructionTypesFilter::All)
                } else {
                    Err(Error::invalid_value(Unexpected::Str(input_str), &self))
                }
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut specifics = Vec::new();
                while let Some(item) = seq.next_element()? {
                    specifics.push(
                        ObstructionType::parse(item)
                            .map_err(|_| Error::invalid_value(Unexpected::Str(item), &self))?,
                    );
                }
                Ok(ObstructionTypesFilter::Specific(specifics))
            }
        }

        deserializer.deserialize_any(ObstructionTypesFilterVisitor)
    }
}

#[derive(
    Debug,
    Eq,
    Serialize,
    Deserialize,
    Hash,
    PartialEq,
    Clone,
    SchemaWrite,
    SchemaRead,
    EnumString,
    AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ObstructionType {
    ActivePermits,
    ApprovedJobApplications,
    NewConstructionCo,
    NewConstructionFootprints,
    RecentJobApplications,
}

pub type ObstructionId = Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

impl ObstructionType {
    pub fn parse(input_str: &str) -> Result<Self, ParseError> {
        ObstructionType::from_str(input_str)
    }
}

impl Display for ObstructionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
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
    _type: String,
    attributes: HashMap<String, AttributeValue>,
    #[serde(rename = "offset_nys")]
    sw_offset: Option<NYSCoords2>,
    x_offset: Option<f64>,
    y_offset: Option<f64>,
    tile_ids: Vec<TileId>,
}

impl ObstructionMeta {
    fn try_from(h: ObstructionMetaDeHelper, type_: ObstructionType) -> Result<Self, String> {
        let sw_offset = match h.sw_offset {
            Some(coords) => coords,
            None => match (h.x_offset, h.y_offset) {
                (Some(x), Some(y)) => NYSCoords2::new(x, y),
                _ => return Err(
                    "missing offset: need either `offset_nys` or both `x_offset` and `y_offset`"
                        .to_string(),
                ),
            },
        };
        Ok(ObstructionMeta {
            id: h.id,
            type_,
            attributes: h.attributes,
            sw_offset,
            tile_ids: h.tile_ids,
        })
    }
}

#[derive(Debug, Serialize, Getters, new)]
pub struct ObstructionMeta {
    #[serde(rename = "obstruction_id")]
    id: ObstructionId,

    #[serde(rename = "obstruction_type")]
    type_: ObstructionType,

    attributes: HashMap<String, AttributeValue>,

    #[serde(rename = "offset_nys")]
    sw_offset: NYSCoords2,

    // Tiles intersected by the footprint
    tile_ids: Vec<TileId>,
}

impl ObstructionMeta {
    pub fn from_json<R>(
        reader: R,
        obstruction_type: ObstructionType,
    ) -> Result<Self, serde_json::Error>
    where
        R: io::Read,
    {
        let obstruction_meta_internal: ObstructionMetaDeHelper = serde_json::from_reader(reader)?;

        // The obstruction types stored inside the JSON files are kinda scrambled, we use
        // the file path as the source of truth to avoid confusion
        ObstructionMeta::try_from(obstruction_meta_internal, obstruction_type)
            .map_err(|e| serde_json::Error::custom(e.to_string()))
    }
}

#[derive(Debug,new)]
pub struct ObstructionRaster {
    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add sw_offset to get global position)
    // Pixels outside the mask=true footprint are set to 0
    heightmap: Array2<u16>,
}

impl ObstructionRaster {

    pub fn heightmap(&self) -> &Array2<u16> {
        &self.heightmap
    }

    pub fn read_from_tiff(
        obstruction_id: ObstructionId,
        file: File,
    ) -> Result<ObstructionRaster, AssetErr> {
        let io = std::io::BufReader::new(file);

        // We would love to use a try here to scope the ?s but it's only available in nightly,
        // so a closure it is
        let inner = move || -> Result<Array2<u16>, Box<dyn std::error::Error>> {
            let mut reader = Decoder::new(io)?;
            let image_data = reader.read_image()?;
            let width: usize = reader.dimensions()?.0.try_into()?;
            let height: usize = reader.dimensions()?.1.try_into()?;

            let colortype = reader.colortype()?;
            if colortype != tiff::ColorType::Gray(16) {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid datatype: {:?}", colortype),
                )));
            }

            if let DecodingResult::U16(image_data) = image_data {
                Ok(Array2::from_shape_vec((height, width), image_data).map_err(Box::new)?)
            } else {
                Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid datatype: {:?}", image_data),
                )))
            }
        };

        let image_data = inner().map_err(|err| AssetErr::AssetContentError(format!(
                "Error parsing obstruction tiff for id {obstruction_id}: {err}"
            )))?;

        Ok(ObstructionRaster {
            heightmap: image_data,
        })
    }

    pub fn write_to_tiff<W: Write + Seek>(&self, mut writer: W) -> Result<(), TiffError> {
        let (height, width) = self.heightmap.dim();
        let mut tiff = TiffEncoder::new(&mut writer)?;
        tiff.write_image::<colortype::Gray16>(
            width as u32,
            height as u32,
            self.heightmap.as_slice().unwrap(),
        )?;
        Ok(())
    }

    pub fn to_obj_stream(
        &self,
        type_: ObstructionType,
        id: ObstructionId,
        x_offset: isize,
        y_offset: isize,
    ) -> impl Stream<Item = String> {
        let heightmap = self.heightmap.clone();
        assert!(heightmap.nrows() < MAX_OBJ_SIZE_USFT);
        assert!(heightmap.ncols() < MAX_OBJ_SIZE_USFT);

        fn_stream(|e| async move {
            e.emit(format!(
                "# Obstruction heightmap terrain\n\
                 # Obstruction id: {id}\n\
                 # Obstruction type: {type_}\n\
                 # X = easting (within tile), Y = northing (within tile), Z = elevation (ft)\n\
                 o heightmap\n\n"
            ))
            .await;

            let mut vi: usize = 0;
            let mut buf = String::with_capacity(16 * 1024);

            for xi in 0..heightmap.nrows() {
                append_obj_row(
                    xi,
                    x_offset,
                    y_offset,
                    &heightmap,
                    &mut vi,
                    &mut buf,
                    |_xi, _yi, z_in| z_in == 0,
                    // Draw side face down to adj_z whenever adj is strictly lower.
                    // OOB neighbors get adj_raw=0, which is always < z_in (pixel is non-zero),
                    // so the face is drawn to the ground — correct for building sides.
                    |_adj_idx, adj_raw, z_in| {
                        if adj_raw >= z_in {
                            None
                        } else {
                            Some(adj_raw as f64 / 12.0)
                        }
                    },
                );
                if buf.len() >= 16 * 1024 {
                    e.emit(std::mem::take(&mut buf)).await;
                }
            }

            if !buf.is_empty() {
                e.emit(buf).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const NEW_STYLE_PAYLOAD: &str = r#"{
        "obstruction_id": "0020fb43-ffab-4083-9bc4-c60d97961d94",
        "obstruction_type": "new_construction_footprints",
        "attributes": {
            "bbl": "3016220051",
            "bin": "3044163",
            "construction_year": 2023,
            "height_roof": 63.0,
            "ground_elevation": 53.0,
            "geom_source": "Other (Manual)",
            "last_status_type": "Constructed"
        },
        "tile_ids": ["2190_31"],
        "offset_nys": [1004021.0, 190791.0],
        "width": 182,
        "height": 80,
        "raster_file": "0020fb43-ffab-4083-9bc4-c60d97961d94.tif"
    }"#;

    #[test]
    fn new_style_payload_deserializes_offset() {
        let meta = ObstructionMeta::from_json(
            Cursor::new(NEW_STYLE_PAYLOAD.as_bytes()),
            ObstructionType::NewConstructionFootprints,
        )
        .unwrap();
        assert_eq!(*meta.sw_offset.easting(), 1004021.0);
        assert_eq!(*meta.sw_offset.northing(), 190791.0);
    }

    #[test]
    fn new_style_payload_deserializes_attributes() {
        let meta = ObstructionMeta::from_json(
            Cursor::new(NEW_STYLE_PAYLOAD.as_bytes()),
            ObstructionType::NewConstructionFootprints,
        )
        .unwrap();
        assert_eq!(meta.attributes.len(), 7);
        assert!(
            matches!(meta.attributes.get("bbl"), Some(AttributeValue::String(s)) if s == "3016220051")
        );
        assert!(matches!(
            meta.attributes.get("height_roof"),
            Some(AttributeValue::Number(_))
        ));
        assert!(matches!(
            meta.attributes.get("construction_year"),
            Some(AttributeValue::Number(_))
        ));
    }

    #[test]
    fn new_style_payload_missing_offset_errors() {
        let payload = r#"{
            "obstruction_id": "0020fb43-ffab-4083-9bc4-c60d97961d94",
            "obstruction_type": "new_construction_footprints",
            "attributes": {},
            "tile_ids": ["2190_31"]
        }"#;
        let result = ObstructionMeta::from_json(
            Cursor::new(payload.as_bytes()),
            ObstructionType::NewConstructionFootprints,
        );
        assert!(result.is_err());
    }

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
        let meta: ObstructionMeta = ObstructionMeta::from_json(
            Cursor::new(LEGACY_PAYLOAD.to_string().into_bytes()),
            ObstructionType::ActivePermits,
        )
        .unwrap();
        assert_eq!(*meta.sw_offset.easting(), 1003728.0);
        assert_eq!(*meta.sw_offset.northing(), 235953.0);
    }

    #[test]
    fn legacy_payload_deserializes_attributes() {
        let meta: ObstructionMeta = ObstructionMeta::from_json(
            Cursor::new(LEGACY_PAYLOAD.to_string().into_bytes()),
            ObstructionType::ActivePermits,
        )
        .unwrap();
        assert_eq!(meta.attributes.len(), 7);

        assert!(
            matches!(meta.attributes.get("bin"), Some(AttributeValue::String(s)) if s == "2129799")
        );
        assert!(matches!(
            meta.attributes.get("ground_elevation"),
            Some(AttributeValue::Number(_))
        ));
        assert!(matches!(
            meta.attributes.get("construction_year"),
            Some(AttributeValue::Number(_))
        ));
    }

    #[test]
    fn legacy_payload_serializes_attributes_back_out() {
        let meta: ObstructionMeta = ObstructionMeta::from_json(
            Cursor::new(LEGACY_PAYLOAD.to_string().into_bytes()),
            ObstructionType::ActivePermits,
        )
        .unwrap();
        let json: serde_json::Value = serde_json::to_value(&meta).unwrap();

        let attrs = json.get("attributes").expect("attributes key missing");
        assert_eq!(attrs.get("bin").and_then(|v| v.as_str()), Some("2129799"));
        assert_eq!(
            attrs.get("ground_elevation").and_then(|v| v.as_f64()),
            Some(34.0)
        );
        assert_eq!(
            attrs.get("construction_year").and_then(|v| v.as_f64()),
            Some(2025.0)
        );
    }

    // --- ObstructionTypesFilter::includes tests ---

    #[test]
    fn includes_all_accepts_any_type() {
        let filter = ObstructionTypesFilter::All;
        assert!(filter.includes(&ObstructionType::ActivePermits));
        assert!(filter.includes(&ObstructionType::NewConstructionCo));
    }

    #[test]
    fn includes_specific_accepts_matching_type() {
        let filter = ObstructionTypesFilter::Specific(vec![ObstructionType::ActivePermits]);
        assert!(filter.includes(&ObstructionType::ActivePermits));
    }

    #[test]
    fn includes_specific_rejects_nonmatching_type() {
        let filter = ObstructionTypesFilter::Specific(vec![ObstructionType::ActivePermits]);
        assert!(!filter.includes(&ObstructionType::NewConstructionCo));
    }

    // --- ObstructionTypesFilter serialize tests ---

    #[test]
    fn serialize_all_produces_star_literal() {
        assert_eq!(
            serde_json::to_string(&ObstructionTypesFilter::All).unwrap(),
            r#""*""#
        );
    }

    #[test]
    fn serialize_specific_produces_array_of_type_strings() {
        let filter = ObstructionTypesFilter::Specific(vec![
            ObstructionType::ActivePermits,
            ObstructionType::NewConstructionCo,
        ]);
        let v: serde_json::Value = serde_json::to_value(&filter).unwrap();
        assert_eq!(
            v,
            serde_json::json!(["active_permits", "new_construction_co"])
        );
    }

    // --- ObstructionTypesFilter deserialize tests ---

    #[test]
    fn deserialize_star_literal_produces_all() {
        let filter: ObstructionTypesFilter = serde_json::from_str(r#""*""#).unwrap();
        assert!(matches!(filter, ObstructionTypesFilter::All));
    }

    #[test]
    fn deserialize_array_produces_specific() {
        let filter: ObstructionTypesFilter =
            serde_json::from_str(r#"["active_permits", "new_construction_co"]"#).unwrap();
        assert!(filter.includes(&ObstructionType::ActivePermits));
        assert!(filter.includes(&ObstructionType::NewConstructionCo));
        assert!(!filter.includes(&ObstructionType::RecentJobApplications));
    }

    #[test]
    fn deserialize_empty_array_produces_specific_with_no_types() {
        let filter: ObstructionTypesFilter = serde_json::from_str("[]").unwrap();
        assert!(!filter.includes(&ObstructionType::ActivePermits));
    }

    #[test]
    fn deserialize_unrecognized_string_errors() {
        assert!(serde_json::from_str::<ObstructionTypesFilter>(r#""all""#).is_err());
        assert!(serde_json::from_str::<ObstructionTypesFilter>(r#""ALL""#).is_err());
    }

    #[test]
    fn roundtrip_all() {
        let json = serde_json::to_string(&ObstructionTypesFilter::All).unwrap();
        let roundtripped: ObstructionTypesFilter = serde_json::from_str(&json).unwrap();
        assert!(matches!(roundtripped, ObstructionTypesFilter::All));
    }

    #[test]
    fn roundtrip_specific() {
        let original = ObstructionTypesFilter::Specific(vec![
            ObstructionType::RecentJobApplications,
            ObstructionType::ActivePermits,
        ]);
        let json = serde_json::to_string(&original).unwrap();
        let roundtripped: ObstructionTypesFilter = serde_json::from_str(&json).unwrap();
        assert!(roundtripped.includes(&ObstructionType::RecentJobApplications));
        assert!(roundtripped.includes(&ObstructionType::ActivePermits));
        assert!(!roundtripped.includes(&ObstructionType::NewConstructionFootprints));
    }
}
