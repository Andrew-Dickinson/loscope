use loscope::providers::elevation_tile_provider::ElevationTile;
use loscope::types::tiles::{LASTileId, SubgridId, TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};
use ndarray::Array2;

use super::classify::{ClassGrid, PixelClass};
use super::rasterize::GRID_SIDE;

pub const TILE_SIDE: usize = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
const GRID_N: usize = 5; // tiles per side of a LAS file (5×5 = 25)

/// Split a 2500×2500 filled uint16 grid into 25 ElevationTiles.
///
/// Only tiles with at least one non-zero pixel are returned; all-zero tiles
/// contain no LiDAR data and should be skipped.
/// Split both the elevation and classification grids into per-tile pairs in a single pass.
///
/// Only tiles with at least one non-zero elevation pixel are returned. Each pair contains
/// the elevation tile and its corresponding 500×500 classification raster.
pub fn split_tiles_with_class(
    filled: &[u16],
    class_grid: &ClassGrid,
    las_id: LASTileId,
) -> Vec<(ElevationTile, Vec<u8>)> {
    let mut tiles = Vec::with_capacity(GRID_N * GRID_N);

    for xi in 0..GRID_N {
        for yi in 0..GRID_N {
            let tile_id = TileId::new(las_id, SubgridId::new(xi as u8, yi as u8));
            let x0 = xi * TILE_SIDE;
            let y0 = yi * TILE_SIDE;

            let mut elev = vec![0u16; TILE_SIDE * TILE_SIDE];
            let mut class_sub = vec![0u8; TILE_SIDE * TILE_SIDE];
            let mut all_zero_elev = true;
            let mut all_water = true;

            for dx in 0..TILE_SIDE {
                for dy in 0..TILE_SIDE {
                    let src = (x0 + dx) * GRID_SIDE + (y0 + dy);
                    let dst = dx * TILE_SIDE + dy;
                    let val = filled[src];
                    let cls = class_grid[src];
                    elev[dst] = val;
                    class_sub[dst] = cls;
                    if val != 0 {
                        all_zero_elev = false;
                    }
                    if cls != PixelClass::Water as u8 {
                        all_water = false;
                    }
                }
            }

            if all_zero_elev && all_water {
                continue;
            }

            let elevation_inches = Array2::from_shape_vec((TILE_SIDE, TILE_SIDE), elev)
                .expect("raster dimensions are always TILE_SIDE × TILE_SIDE");
            tiles.push((ElevationTile::new(tile_id, elevation_inches), class_sub));
        }
    }

    tiles
}

/// Split a 2500×2500 filled uint16 grid into 25 ElevationTiles.
///
/// Only tiles with at least one non-zero pixel are returned; all-zero tiles
/// contain no LiDAR data and should be skipped.
pub fn split_tiles(filled: &[u16], las_id: LASTileId) -> Vec<ElevationTile> {
    let mut tiles = Vec::with_capacity(GRID_N * GRID_N);
    for xi in 0..GRID_N {
        for yi in 0..GRID_N {
            let tile_id = TileId::new(las_id, SubgridId::new(xi as u8, yi as u8));
            let x0 = xi * TILE_SIDE;
            let y0 = yi * TILE_SIDE;
            let mut raster = vec![0u16; TILE_SIDE * TILE_SIDE];
            let mut any_nonzero = false;
            for dx in 0..TILE_SIDE {
                for dy in 0..TILE_SIDE {
                    let val = filled[(x0 + dx) * GRID_SIDE + (y0 + dy)];
                    raster[dx * TILE_SIDE + dy] = val;
                    if val != 0 { any_nonzero = true; }
                }
            }
            if !any_nonzero { continue; }
            let elevation_inches = Array2::from_shape_vec((TILE_SIDE, TILE_SIDE), raster)
                .expect("raster dimensions are always TILE_SIDE × TILE_SIDE");
            tiles.push(ElevationTile::new(tile_id, elevation_inches));
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_class_subtile(class_grid: &ClassGrid, xi: usize, yi: usize) -> Vec<u8> {
        let x0 = xi * TILE_SIDE;
        let y0 = yi * TILE_SIDE;
        let mut out = vec![0u8; TILE_SIDE * TILE_SIDE];
        for dx in 0..TILE_SIDE {
            for dy in 0..TILE_SIDE {
                let src = (x0 + dx) * GRID_SIDE + (y0 + dy);
                out[dx * TILE_SIDE + dy] = class_grid[src];
            }
        }
        out
    }

    fn make_las_id() -> LASTileId {
        // 500300 is a valid NYC LAS tile ID used in backend-rs tests.
        LASTileId::parse("500300").unwrap()
    }

    fn all_water_class() -> ClassGrid {
        vec![PixelClass::Water as u8; GRID_SIDE * GRID_SIDE]
    }

    #[test]
    fn all_zero_elev_all_water_produces_no_tiles() {
        let filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        let class = all_water_class();
        let pairs = split_tiles_with_class(&filled, &class, make_las_id());
        assert!(pairs.is_empty());
    }

    #[test]
    fn all_zero_elev_but_not_all_water_is_kept() {
        let filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        // One pixel classified as None means the tile is not pure water → keep it
        let mut class = all_water_class();
        class[0] = PixelClass::None as u8;
        let pairs = split_tiles_with_class(&filled, &class, make_las_id());
        assert!(!pairs.is_empty());
    }

    #[test]
    fn nonzero_elev_all_water_is_kept() {
        // A tile over a river may have valid elevation (bathymetry / bridge) and all-water class
        let mut filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        filled[0] = 100;
        let class = all_water_class();
        let pairs = split_tiles_with_class(&filled, &class, make_las_id());
        assert!(!pairs.is_empty());
    }

    #[test]
    fn single_nonzero_pixel_produces_one_tile() {
        let mut filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        // Place a nonzero value in the first subgrid (xi=0, yi=0).
        filled[0] = 100;
        let tiles = split_tiles(&filled, make_las_id());
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].elevation_inches()[[0, 0]], 100);
    }

    #[test]
    fn full_grid_produces_twenty_five_tiles() {
        let filled = vec![1u16; GRID_SIDE * GRID_SIDE];
        let tiles = split_tiles(&filled, make_las_id());
        assert_eq!(tiles.len(), 25);
    }

    #[test]
    fn tile_id_matches_subgrid() {
        let mut filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        // Activate subgrid xi=2, yi=3 only.
        let x0 = 2 * TILE_SIDE;
        let y0 = 3 * TILE_SIDE;
        filled[x0 * GRID_SIDE + y0] = 42;

        let tiles = split_tiles(&filled, make_las_id());
        assert_eq!(tiles.len(), 1);
        assert_eq!(*tiles[0].id(), TileId::new(make_las_id(), SubgridId::new(2, 3)));
    }

    #[test]
    fn extract_class_subtile_pulls_correct_region() {
        let mut class_grid = vec![0u8; GRID_SIDE * GRID_SIDE];
        // Mark a single pixel in sub-tile (xi=1, yi=2) at local offset (3, 7)
        let x0 = TILE_SIDE + 3;
        let y0 = 2 * TILE_SIDE + 7;
        class_grid[x0 * GRID_SIDE + y0] = 2; // Building

        let sub = extract_class_subtile(&class_grid, 1, 2);
        assert_eq!(sub.len(), TILE_SIDE * TILE_SIDE);
        assert_eq!(sub[3 * TILE_SIDE + 7], 2);
        assert!(sub.iter().enumerate().all(|(i, &v)| i == 3 * TILE_SIDE + 7 || v == 0));
    }

    #[test]
    fn extract_class_subtile_different_subgrids_dont_overlap() {
        let mut class_grid = vec![0u8; GRID_SIDE * GRID_SIDE];
        // Paint sub-tile (0,0) completely with 1 and sub-tile (1,0) completely with 2
        for dx in 0..TILE_SIDE {
            for dy in 0..TILE_SIDE {
                class_grid[dx * GRID_SIDE + dy] = 1;
                class_grid[(TILE_SIDE + dx) * GRID_SIDE + dy] = 2;
            }
        }
        let sub00 = extract_class_subtile(&class_grid, 0, 0);
        let sub10 = extract_class_subtile(&class_grid, 1, 0);
        assert!(sub00.iter().all(|&v| v == 1));
        assert!(sub10.iter().all(|&v| v == 2));
    }
}