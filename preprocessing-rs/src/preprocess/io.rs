use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use loscope::providers::elevation_tile_provider::ElevationTile;
use loscope::types::tiles::{TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};
use tiff::encoder::{TiffEncoder, colortype};

/// Write an ElevationTile as a Gray16 TIFF at `{out_dir}/{tile_id}.tif`.
pub fn write_tile(tile: &ElevationTile, out_dir: &Path) -> Result<()> {
    let path = out_dir.join(tile.id().tiff_fname());
    let file = File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    tile.write_to_tiff(BufWriter::new(file))
        .with_context(|| format!("Failed to write TIFF {}", path.display()))
}

/// Write a classification tile as a Gray8 TIFF at `{out_dir}/{tile_id}-class.tif`.
///
/// Pixel values: 0 = None, 1 = Vegetation, 2 = Building, 3 = Water.
pub fn write_class_tile(tile_id: &TileId, data: &[u8], out_dir: &Path) -> Result<()> {
    let path = out_dir.join(tile_id.tiff_fname().replace(".tif", "-class.tif"));
    let file = File::create(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    let side: u32 = SUBGRID_TILE_SIDE_LENGTH_USFT.into();
    let mut tiff = TiffEncoder::new(BufWriter::new(file))
        .with_context(|| format!("Failed to init TIFF encoder for {}", path.display()))?;
    tiff.write_image::<colortype::Gray8>(side, side, data)
        .with_context(|| format!("Failed to write class TIFF {}", path.display()))?;
    Ok(())
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
    use tiff::ColorType;

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

    #[test]
    fn write_class_tile_round_trips() -> Result<()> {
        let dir = tempdir()?;
        let las_id = LASTileId::parse("500300").unwrap();
        let tile_id = TileId::new(las_id, SubgridId::new(1, 2));

        let mut data = vec![0u8; TILE_SIDE * TILE_SIDE];
        data[0] = 1; // Vegetation
        data[TILE_SIDE * TILE_SIDE - 1] = 3; // Water

        write_class_tile(&tile_id, &data, dir.path())?;

        let tif_path = dir.path().join(tile_id.tiff_fname().replace(".tif", "-class.tif"));
        assert!(tif_path.exists());

        let file = File::open(&tif_path)?;
        let mut decoder = Decoder::new(file)?;
        let (w, h) = decoder.dimensions()?;
        assert_eq!(w, TILE_SIDE as u32);
        assert_eq!(h, TILE_SIDE as u32);
        assert_eq!(decoder.colortype()?, ColorType::Gray(8));

        let DecodingResult::U8(pixels) = decoder.read_image()? else {
            panic!("expected u8 image");
        };
        assert_eq!(pixels[0], 1);
        assert_eq!(pixels[TILE_SIDE * TILE_SIDE - 1], 3);

        Ok(())
    }
}