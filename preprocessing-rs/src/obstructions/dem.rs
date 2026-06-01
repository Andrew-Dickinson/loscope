use std::fs::File;
use std::path::Path;

use geo::{Contains, Point, Polygon};
use loscope::building::heightmap::get_intersecting_tiles;
use loscope::types::tiles::TileId;
use tiff::decoder::{Decoder, DecodingResult};

pub struct DemTile {
    tile_id: TileId,
    pixels: Vec<u16>,
    width: usize,
    height: usize,
}

impl DemTile {
    pub fn read(tile_id: TileId, path: &Path) -> Option<DemTile> {
        let mut decoder = Decoder::new(File::open(path).ok()?).ok()?;
        let (w, h) = decoder.dimensions().ok()?;
        let DecodingResult::U16(pixels) = decoder.read_image().ok()? else {
            return None;
        };
        Some(DemTile { tile_id, pixels, width: w as usize, height: h as usize })
    }

    pub fn max_elevation_inside(&self, poly: &Polygon<f64>) -> Option<f64> {
        let sw = self.tile_id.get_sw_corner();
        let e_sw = *sw.easting();
        let n_sw = *sw.northing();
        let mut max_inches: Option<u32> = None;

        for x in 0..self.width {
            for y in 0..self.height {
                let center = Point::new(e_sw + x as f64 + 0.5, n_sw + y as f64 + 0.5);
                if poly.contains(&center) {
                    let val = self.pixels[x * self.height + y] as u32;
                    max_inches = Some(max_inches.map_or(val, |m| m.max(val)));
                }
            }
        }

        max_inches.map(|inches| inches as f64 / 12.0)
    }
}

/// Return the maximum ground elevation (usft) for a polygon using local DEM tiles.
///
/// Reads `{dem_cache}/{tile_id}.tif` for every tile that intersects the polygon.
/// Returns `None` when no local DEM tiles cover the polygon.
pub fn max_ground_elevation_from_dem(poly: &Polygon<f64>, dem_cache: &Path) -> Option<f64> {
    let tile_ids = get_intersecting_tiles(poly).ok().map(|(t, _)| t).unwrap_or_default();

    tile_ids.into_iter()
        .filter_map(|tile_id| DemTile::read(tile_id, &dem_cache.join(tile_id.tiff_fname())))
        .filter_map(|tile| tile.max_elevation_inside(poly))
        .reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;
    use loscope::types::tiles::{LASTileId, SubgridId};
    use tempfile::tempdir;
    use tiff::encoder::{TiffEncoder, colortype};

    fn write_dem_tiff(dir: &Path, tile_id: TileId, pixels: &[u16], w: u32, h: u32) {
        let tif = File::create(dir.join(tile_id.tiff_fname())).unwrap();
        let mut enc = TiffEncoder::new(std::io::BufWriter::new(tif)).unwrap();
        enc.write_image::<colortype::Gray16>(w, h, pixels).unwrap();
    }

    fn tile_500235_00() -> TileId {
        TileId::new(LASTileId::parse("500235").unwrap(), SubgridId::new(0, 0))
    }

    #[test]
    fn returns_max_pixel_inside_polygon() {
        let dir = tempdir().unwrap();

        // 4×4 DEM tile at NYS coords (500_000, 235_000).
        let pixels = vec![
            100u16, 200, 300, 400,
            500,    600, 700, 800,
            900,   1000, 1100, 1200,
            1300,  1400, 1500, 1600,
        ];
        write_dem_tiff(dir.path(), tile_500235_00(), &pixels, 4, 4);

        // Polygon covering only the first pixel (centre at 500000.5, 235000.5).
        let poly: Polygon<f64> = polygon![
            (x: 500_000.0, y: 235_000.0),
            (x: 500_001.0, y: 235_000.0),
            (x: 500_001.0, y: 235_001.0),
            (x: 500_000.0, y: 235_001.0),
            (x: 500_000.0, y: 235_000.0),
        ];

        let result = max_ground_elevation_from_dem(&poly, dir.path());
        // pixel[0][0] = 100 inches = 100/12 usft ≈ 8.33
        assert!(result.is_some());
        let elev = result.unwrap();
        assert!((elev - 100.0 / 12.0).abs() < 0.01, "expected ~8.33, got {elev}");
    }

    #[test]
    fn returns_none_when_no_dem_files() {
        let dir = tempdir().unwrap();
        let poly: Polygon<f64> = polygon![
            (x: 500_000.0, y: 235_000.0), (x: 500_001.0, y: 235_000.0),
            (x: 500_001.0, y: 235_001.0), (x: 500_000.0, y: 235_001.0),
            (x: 500_000.0, y: 235_000.0),
        ];
        assert!(max_ground_elevation_from_dem(&poly, dir.path()).is_none());
    }
}