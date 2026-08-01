use ndarray::s;
use uuid::Uuid;

use loscope::types::coords::NYSCoords2;
use loscope::types::obstructions::{MAX_OBSTRUCTION_RASTER_BYTES, ObstructionRaster};

use super::model::ObstructionMetaOutput;

/// If `raster` exceeds `MAX_OBSTRUCTION_RASTER_BYTES` (uncompressed, 2 bytes/pixel), splits it
/// into one sub-obstruction per intersecting 500x500 elevation tile instead of writing a single
/// giant raster. Some tax lots (e.g. Central Park) span hundreds of tiles, producing rasters
/// hundreds of MB in size -- far bigger than any downstream consumer expects an obstruction to
/// be. `ObstructionRaster::read_from_tiff` enforces the same cap on read, as a backstop.
///
/// Returns one `(meta, raster)` pair per tile in `meta.tile_ids`, each cropped to that tile's
/// bounds and tagged with a shared `obstruction_group_id` (set to `meta.obstruction_id`). Tiles
/// whose cropped region ends up entirely zero-valued are dropped. Returns `None` when no split is
/// needed, leaving the caller to write `meta`/`raster` unchanged.
pub fn maybe_split(
    meta: &ObstructionMetaOutput,
    raster: &ObstructionRaster,
) -> Option<Vec<(ObstructionMetaOutput, ObstructionRaster)>> {
    let heightmap = raster.heightmap();
    // Axes are [easting_local, northing_local]: dim().0 is the width (easting) extent,
    // dim().1 is the height (northing) extent -- see ObstructionRaster's doc comment.
    let (raster_w, raster_h) = heightmap.dim();
    if (raster_w * raster_h * 2) as u64 <= MAX_OBSTRUCTION_RASTER_BYTES {
        return None;
    }

    let ox = meta.offset_nys.easting().round() as i64;
    let oy = meta.offset_nys.northing().round() as i64;
    let group_id = meta.obstruction_id;

    let mut chunks = Vec::new();
    for tile in &meta.tile_ids {
        let bounds = tile.get_bounds();
        let tile_x0 = i64::from(bounds.min().x);
        let tile_y0 = i64::from(bounds.min().y);
        let tile_x1 = i64::from(bounds.max().x);
        let tile_y1 = i64::from(bounds.max().y);

        let x0 = ox.max(tile_x0);
        let x1 = (ox + raster_w as i64).min(tile_x1);
        let y0 = oy.max(tile_y0);
        let y1 = (oy + raster_h as i64).min(tile_y1);

        if x1 <= x0 || y1 <= y0 {
            continue;
        }

        let crop_w = (x1 - x0) as usize;
        let crop_h = (y1 - y0) as usize;
        let src_x0 = (x0 - ox) as usize;
        let src_y0 = (y0 - oy) as usize;

        let cropped = heightmap
            .slice(s![src_x0..src_x0 + crop_w, src_y0..src_y0 + crop_h])
            .to_owned();

        if cropped.iter().all(|&v| v == 0) {
            continue;
        }

        let chunk_id = Uuid::new_v4();
        chunks.push((
            ObstructionMetaOutput {
                obstruction_id: chunk_id,
                obstruction_type: meta.obstruction_type.clone(),
                attributes: meta.attributes.clone(),
                tile_ids: vec![*tile],
                offset_nys: NYSCoords2::new(x0 as f64, y0 as f64),
                width: crop_w,
                height: crop_h,
                raster_file: format!("{chunk_id}.tif"),
                obstruction_group_id: Some(group_id),
            },
            ObstructionRaster::new(cropped),
        ));
    }

    Some(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loscope::types::obstructions::{AttributeValue, ObstructionType};
    use loscope::types::tiles::TileId;
    use ndarray::Array2;
    use std::collections::HashMap;

    fn meta_for(
        offset_nys: NYSCoords2,
        width: usize,
        height: usize,
        tile_ids: Vec<TileId>,
    ) -> ObstructionMetaOutput {
        let mut attributes = HashMap::new();
        attributes.insert("bin".to_string(), AttributeValue::String("1234567".to_string()));

        ObstructionMetaOutput {
            obstruction_id: Uuid::new_v4(),
            obstruction_type: ObstructionType::NewConstructionFootprints,
            attributes,
            tile_ids,
            offset_nys,
            width,
            height,
            raster_file: "unused.tif".to_string(),
            obstruction_group_id: None,
        }
    }

    #[test]
    fn small_raster_is_not_split() {
        // 100x100 px * 2 bytes = 20,000 bytes, well under the 1 MiB threshold.
        let raster = ObstructionRaster::new(Array2::from_elem((100, 100), 500u16));
        let meta = meta_for(
            NYSCoords2::new(500_000.0, 300_000.0),
            100,
            100,
            vec![TileId::parse("500300_00").unwrap()],
        );

        assert!(maybe_split(&meta, &raster).is_none());
    }

    // Three adjacent 500x500 tiles along the easting axis, within one LAS tile.
    fn three_tiles_in_a_row() -> Vec<TileId> {
        vec![
            TileId::parse("500300_00").unwrap(),
            TileId::parse("500300_10").unwrap(),
            TileId::parse("500300_20").unwrap(),
        ]
    }

    #[test]
    fn oversized_raster_splits_one_chunk_per_tile() {
        // Raster spans three adjacent 500x500 tiles along the easting axis: 1500x500 px *
        // 2 bytes = 1,500,000 bytes, over the 1 MiB threshold.
        let width = 1500;
        let height = 500;
        let raster = ObstructionRaster::new(Array2::from_elem((width, height), 720u16));
        let sw = TileId::parse("500300_00").unwrap().get_sw_corner();
        let (ox, oy) = (*sw.easting(), *sw.northing());
        let meta = meta_for(NYSCoords2::new(ox, oy), width, height, three_tiles_in_a_row());

        let chunks = maybe_split(&meta, &raster).expect("raster should be split");
        assert_eq!(chunks.len(), 3);

        for (chunk_meta, chunk_raster) in &chunks {
            assert_eq!(chunk_meta.obstruction_group_id, Some(meta.obstruction_id));
            assert_eq!(chunk_meta.tile_ids.len(), 1);
            assert_eq!(chunk_meta.attributes.len(), meta.attributes.len());
            assert!(
                matches!(chunk_meta.attributes.get("bin"), Some(AttributeValue::String(s)) if s == "1234567")
            );
            assert_eq!(chunk_meta.obstruction_type, meta.obstruction_type);
            assert_eq!(chunk_meta.width, 500);
            assert_eq!(chunk_meta.height, 500);
            assert!(chunk_raster.heightmap().iter().all(|&v| v == 720));
            // Every chunk gets its own fresh id, distinct from the shared group id.
            assert_ne!(chunk_meta.obstruction_id, meta.obstruction_id);
        }

        let ids: std::collections::HashSet<_> =
            chunks.iter().map(|(m, _)| m.obstruction_id).collect();
        assert_eq!(ids.len(), 3, "each chunk should have a unique obstruction_id");

        let offsets: std::collections::HashSet<_> = chunks
            .iter()
            .map(|(m, _)| (*m.offset_nys.easting() as i64, *m.offset_nys.northing() as i64))
            .collect();
        assert!(offsets.contains(&(ox as i64, oy as i64)));
        assert!(offsets.contains(&(ox as i64 + 500, oy as i64)));
        assert!(offsets.contains(&(ox as i64 + 1000, oy as i64)));
    }

    #[test]
    fn tile_with_no_overlap_is_skipped() {
        // A fourth, far-away tile_id is included to verify it produces no chunk instead of an
        // empty/degenerate one.
        let width = 1500;
        let height = 500;
        let raster = ObstructionRaster::new(Array2::from_elem((width, height), 42u16));
        let sw = TileId::parse("500300_00").unwrap().get_sw_corner();
        let (ox, oy) = (*sw.easting(), *sw.northing());
        let mut tile_ids = three_tiles_in_a_row();
        tile_ids.push(TileId::parse("990200_44").unwrap()); // far away, no overlap
        let meta = meta_for(NYSCoords2::new(ox, oy), width, height, tile_ids);

        let chunks = maybe_split(&meta, &raster).expect("raster should be split");
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn all_zero_chunk_is_dropped() {
        // First tile's region (x 0..500) is all zero; the other two are non-zero.
        let width = 1500;
        let height = 500;
        let mut arr = Array2::from_elem((width, height), 0u16);
        for xi in 500..width {
            for yi in 0..height {
                arr[[xi, yi]] = 99;
            }
        }
        let raster = ObstructionRaster::new(arr);
        let sw = TileId::parse("500300_00").unwrap().get_sw_corner();
        let (ox, oy) = (*sw.easting(), *sw.northing());
        let meta = meta_for(NYSCoords2::new(ox, oy), width, height, three_tiles_in_a_row());

        let chunks = maybe_split(&meta, &raster).expect("raster should be split");
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|(_, r)| r.heightmap().iter().all(|&v| v == 99)));
    }
}
