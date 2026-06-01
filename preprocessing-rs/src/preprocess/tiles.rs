use loscope::providers::elevation_tile_provider::ElevationTile;
use loscope::types::tiles::{LASTileId, SubgridId, TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};
use ndarray::Array2;

use super::rasterize::GRID_SIDE;

const TILE_SIDE: usize = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
const GRID_N: usize = 5; // tiles per side of a LAS file (5×5 = 25)

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
                    let src_idx = (x0 + dx) * GRID_SIDE + (y0 + dy);
                    let val = filled[src_idx];
                    raster[dx * TILE_SIDE + dy] = val;
                    if val != 0 {
                        any_nonzero = true;
                    }
                }
            }

            if !any_nonzero {
                continue;
            }

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

    fn make_las_id() -> LASTileId {
        // 500300 is a valid NYC LAS tile ID used in backend-rs tests.
        LASTileId::parse("500300").unwrap()
    }

    #[test]
    fn all_zero_grid_produces_no_tiles() {
        let filled = vec![0u16; GRID_SIDE * GRID_SIDE];
        let tiles = split_tiles(&filled, make_las_id());
        assert!(tiles.is_empty());
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
}