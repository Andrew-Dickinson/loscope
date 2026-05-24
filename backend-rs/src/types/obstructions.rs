use std::collections::HashMap;
use std::iter::Map;
use ndarray::Array2;
use rocket::serde::{Deserialize, Serialize};
use wincode::{SchemaRead, SchemaWrite};
use crate::types::coords::NYSCoords2;
use crate::types::tiles::TileId;

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

pub type ObstructionType = String;
pub type ObstructionId = String;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AttributeValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}


pub struct Obstruction {
    id: ObstructionId,
    type_: ObstructionType,
    attributes: HashMap<String, AttributeValue>,
    sw_offset: NYSCoords2,

    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add sw_offset to get global position)
    // Pixels outside the mask=true footprint are set to 0
    heightmap: Array2<u16>,
    mask: Array2<bool>,

    // Tiles intersected by the footprint
    tile_ids: Vec<TileId>
}
