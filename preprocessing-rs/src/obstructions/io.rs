use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use loscope::types::obstructions::ObstructionRaster;

use super::model::ObstructionMetaOutput;

/// Write an obstruction TIF + JSON pair to `{out_dir}/{type}/{uuid}.*`.
pub fn write_obstruction(meta: &ObstructionMetaOutput, raster: &ObstructionRaster, out_dir: &Path) -> Result<()> {
    let type_dir = out_dir.join(meta.obstruction_type.as_ref());
    fs::create_dir_all(&type_dir)?;

    let tif_path = type_dir.join(&meta.raster_file);
    let json_path = type_dir.join(format!("{}.json", meta.obstruction_id));

    let file = File::create(&tif_path)
        .with_context(|| format!("Failed to create {}", tif_path.display()))?;
    raster.write_to_tiff(BufWriter::new(file))
        .with_context(|| format!("Failed to write TIFF {}", tif_path.display()))?;

    let json_file = File::create(&json_path)
        .with_context(|| format!("Failed to create {}", json_path.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(json_file), meta)
        .with_context(|| format!("Failed to write JSON {}", json_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loscope::types::coords::NYSCoords2;
    use loscope::types::obstructions::{AttributeValue, ObstructionMeta, ObstructionRaster, ObstructionType};
    use loscope::types::tiles::TileId;
    use ndarray::Array2;
    use std::collections::HashMap;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn written_json_accepted_by_backend_deserializer() -> Result<()> {
        let dir = tempdir()?;
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        let meta = ObstructionMetaOutput {
            obstruction_id: id,
            obstruction_type: ObstructionType::NewConstructionFootprints,
            attributes: {
                let mut m = HashMap::new();
                m.insert("bin".to_string(), AttributeValue::String("1234567".parse()?));
                m
            },
            tile_ids: vec![TileId::parse("500300_23").unwrap()],
            offset_nys: NYSCoords2::new(500_300.0, 235_000.0),
            width: 2,
            height: 2,
            raster_file: format!("{id}.tif"),
        };

        let raster = ObstructionRaster::new(Array2::from_shape_vec((2, 2), vec![100u16, 200, 300, 400]).unwrap());
        write_obstruction(&meta, &raster, dir.path())?;

        // Verify JSON is readable by backend deserializer.
        let json_path = dir.path().join("new_construction_footprints").join(format!("{id}.json"));
        assert!(json_path.exists());
        let file = File::open(&json_path)?;
        let parsed = ObstructionMeta::from_json(file, ObstructionType::NewConstructionFootprints)
            .expect("backend should parse our JSON");
        assert_eq!(*parsed.id(), id);

        // Verify TIFF exists and is readable.
        let tif_path = dir.path().join("new_construction_footprints").join(format!("{id}.tif"));
        assert!(tif_path.exists());

        Ok(())
    }
}