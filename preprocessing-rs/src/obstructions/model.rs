use std::collections::HashMap;

use loscope::types::coords::NYSCoords2;
use serde::Serialize;
use loscope::types::obstructions::{AttributeValue, ObstructionId, ObstructionType};
use loscope::types::tiles::TileId;

/// Serializable output struct matching the JSON schema read by `ObstructionMeta::from_json`.
#[derive(Debug, Serialize)]
pub struct ObstructionMetaOutput {
    pub obstruction_id: ObstructionId,
    pub obstruction_type: ObstructionType,
    pub attributes: HashMap<String, AttributeValue>,
    pub tile_ids: Vec<TileId>,
    pub offset_nys: NYSCoords2,
    pub width: usize,
    pub height: usize,
    pub raster_file: String,
    // Shared by every sub-obstruction produced when a too-large raster is split into one
    // obstruction per tile (see obstructions::split); unset for non-split obstructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obstruction_group_id: Option<ObstructionId>,
}

#[cfg(test)]
mod tests {
    use serde_json::Number;
    use super::*;
    use loscope::types::obstructions::{ObstructionMeta, ObstructionType};
    use loscope::types::tiles::TileId;
    use uuid::Uuid;

    #[test]
    fn output_round_trips_through_backend_deserializer() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let output = ObstructionMetaOutput {
            obstruction_id: id,
            obstruction_type: ObstructionType::NewConstructionFootprints,
            attributes: {
                let mut m = HashMap::new();
                m.insert("bin".to_string(), AttributeValue::String("1234567".parse().unwrap()));
                m.insert("height_roof".to_string(), AttributeValue::Number(Number::from_f64(120.5).unwrap()));
                m
            },
            tile_ids: vec![TileId::parse("500300_23").unwrap()],
            offset_nys: NYSCoords2::new(500_300.0, 235_000.0),
            width: 10,
            height: 10,
            raster_file: format!("{id}.tif"),
            obstruction_group_id: None,
        };

        let json = serde_json::to_string(&output).expect("serialization failed");
        let cursor = std::io::Cursor::new(json.as_bytes());

        let meta = ObstructionMeta::from_json(cursor, ObstructionType::NewConstructionFootprints)
            .expect("backend deserializer should accept our JSON output");

        assert_eq!(*meta.id(), id);
        assert_eq!(*meta.sw_offset().easting(), 500_300.0);
        assert_eq!(*meta.sw_offset().northing(), 235_000.0);
        assert_eq!(meta.tile_ids().len(), 1);
    }
}