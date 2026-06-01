use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use loscope::providers::elevation_tile_provider::ElevationTile;

/// Write an ElevationTile as a Gray16 TIFF at `{out_dir}/{tile_id}.tif`.
pub fn write_tile(tile: &ElevationTile, out_dir: &Path) -> Result<()> {
    let path = out_dir.join(tile.id().tiff_fname());
    let file = File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    tile.write_to_tiff(BufWriter::new(file))
        .with_context(|| format!("Failed to write TIFF {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preprocess::tiles::TILE_SIDE;
    use loscope::types::tiles::{SubgridId, TileId};
    use loscope::types::tiles::LASTileId;
    use ndarray::Array2;
    use tempfile::tempdir;
    use tiff::decoder::{Decoder, DecodingResult};

    #[test]
    fn round_trip_write_and_read() -> Result<()> {
        let dir = tempdir()?;
        let las_id = LASTileId::parse("500300").unwrap();
        let tile_id = TileId::new(las_id, SubgridId::new(2, 3));

        let mut raster = vec![0u16; TILE_SIDE * TILE_SIDE];
        raster[0] = 1234;
        raster[TILE_SIDE - 1] = 5678;
        raster[TILE_SIDE * TILE_SIDE - 1] = 9999;

        let elevation_inches = Array2::from_shape_vec((TILE_SIDE, TILE_SIDE), raster.clone())
            .unwrap();
        let tile = ElevationTile::new(tile_id, elevation_inches);

        write_tile(&tile, dir.path())?;

        let tif_path = dir.path().join(tile_id.tiff_fname());
        assert!(tif_path.exists());

        let file = File::open(&tif_path)?;
        let mut decoder = Decoder::new(file)?;
        let (w, h) = decoder.dimensions()?;
        assert_eq!(w, TILE_SIDE as u32);
        assert_eq!(h, TILE_SIDE as u32);

        let DecodingResult::U16(pixels) = decoder.read_image()? else {
            panic!("expected u16 image");
        };

        assert_eq!(pixels[0], raster[0]);
        assert_eq!(pixels[TILE_SIDE - 1], raster[TILE_SIDE - 1]);
        assert_eq!(pixels[TILE_SIDE * TILE_SIDE - 1], raster[TILE_SIDE * TILE_SIDE - 1]);

        Ok(())
    }
}