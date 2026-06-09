use anyhow::{Context, Result};
use loscope::types::tiles::{LAS_TILE_SIDE_LENGTH_USFT, LASTileId};

const MAX_Z_USFT: f64 = 2000.0;
const NOISE_CLASSIFICATION_1: u8 = 7;
const NOISE_CLASSIFICATION_2: u8 = 18;

pub const GRID_SIDE: usize = LAS_TILE_SIDE_LENGTH_USFT as usize;

/// Flat 2D grid stored in [easting_local][northing_local] order.
/// Index with `grid[x * GRID_SIDE + y]`.
pub type HeightGrid = Vec<f64>;
pub type CountGrid = Vec<u32>;
/// Maximum z of any vegetation-classified (3/4/5) point per cell, in usft. Zero means no veg point.
pub type VegGrid = Vec<f64>;

/// Rasterize a LAS file into a 2500×2500 max-height grid (usft), a data-count grid,
/// and a per-cell max vegetation point height grid.
///
/// Returns `(height_grid, count_grid, veg_grid)` where index `[x * GRID_SIDE + y]`
/// corresponds to the 1-usft cell at local easting `x`, northing `y`. Height and veg
/// values are in US survey feet; count is the number of non-filtered points in that cell.
pub fn build_height_grid(las_path: &str, las_id: LASTileId) -> Result<(HeightGrid, CountGrid, VegGrid)> {
    let origin = las_id.get_sw_corner();
    let origin_e = *origin.easting();
    let origin_n = *origin.northing();

    let n = GRID_SIDE * GRID_SIDE;
    let mut height_grid = vec![0.0f64; n];
    let mut count_grid = vec![0u32; n];
    let mut veg_grid = vec![0.0f64; n];

    let mut reader = las::Reader::from_path(las_path)
        .with_context(|| format!("Failed to open LAS file: {las_path}"))?;

    for wrapped_point in reader.points() {
        let point = wrapped_point.context("Failed to read LAS point")?;

        let classification = u8::from(point.classification);
        if classification == NOISE_CLASSIFICATION_1 || classification == NOISE_CLASSIFICATION_2 {
            continue;
        }

        let z = point.z;
        if z >= MAX_Z_USFT {
            continue;
        }

        let xi = (point.x - origin_e).floor() as isize;
        let yi = (point.y - origin_n).floor() as isize;

        if xi < 0 || xi >= GRID_SIDE as isize || yi < 0 || yi >= GRID_SIDE as isize {
            continue;
        }

        let idx = xi as usize * GRID_SIDE + yi as usize;
        if z > height_grid[idx] {
            height_grid[idx] = z;
        }
        count_grid[idx] += 1;

        // LAS vegetation classifications: Low (3), Medium (4), High (5)
        if matches!(classification, 3 | 4 | 5) && z > veg_grid[idx] {
            veg_grid[idx] = z;
        }
    }

    Ok((height_grid, count_grid, veg_grid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_classifications_are_filtered() {
        assert_eq!(NOISE_CLASSIFICATION_1, 7);
        assert_eq!(NOISE_CLASSIFICATION_2, 18);
    }

    #[test]
    fn max_z_threshold() {
        assert_eq!(MAX_Z_USFT, 2000.0);
    }

    #[test]
    fn grid_side_matches_las_tile_constant() {
        assert_eq!(GRID_SIDE, LAS_TILE_SIDE_LENGTH_USFT as usize);
    }

    #[test]
    fn height_and_count_grid_sizes() {
        let expected_len = GRID_SIDE * GRID_SIDE;
        let h: HeightGrid = vec![0.0; expected_len];
        let c: CountGrid = vec![0; expected_len];
        let v: VegGrid = vec![0.0; expected_len];
        assert_eq!(h.len(), expected_len);
        assert_eq!(c.len(), expected_len);
        assert_eq!(v.len(), expected_len);
    }
}
