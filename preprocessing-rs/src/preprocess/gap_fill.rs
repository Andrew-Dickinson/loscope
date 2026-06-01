use super::rasterize::{CountGrid, GRID_SIDE, HeightGrid};

/// Apply a 3×3 median-filter gap fill and convert heights from usft to uint16 inches.
///
/// Pixels where `count > 0` keep their original height. Zero-count pixels are filled
/// from the 3×3 neighbourhood median. Heights are converted to inches and clamped to
/// u16 range. The returned flat vec has the same `[x * GRID_SIDE + y]` layout.
pub fn fill_gaps(height_grid: &HeightGrid, count_grid: &CountGrid) -> Vec<u16> {
    let n = GRID_SIDE * GRID_SIDE;
    let mut result = vec![0u16; n];
    for x in 0..GRID_SIDE {
        for y in 0..GRID_SIDE {
            let idx = x * GRID_SIDE + y;
            let h = if count_grid[idx] > 0 {
                height_grid[idx]
            } else {
                neighbourhood_median(height_grid, x, y)
            };
            result[idx] = (h * 12.0).round().clamp(0.0, 65535.0) as u16;
        }
    }
    result
}

/// Median of valid (non-zero-height) neighbours in a 3×3 window.
fn neighbourhood_median(grid: &HeightGrid, x: usize, y: usize) -> f64 {
    let mut values: Vec<f64> = Vec::with_capacity(9);
    let x0 = x.saturating_sub(1);
    let x1 = (x + 1).min(GRID_SIDE - 1);
    let y0 = y.saturating_sub(1);
    let y1 = (y + 1).min(GRID_SIDE - 1);

    for nx in x0..=x1 {
        for ny in y0..=y1 {
            let v = grid[nx * GRID_SIDE + ny];
            if v > 0.0 {
                values.push(v);
            }
        }
    }

    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grids(size: usize) -> (HeightGrid, CountGrid) {
        (vec![0.0; size * size], vec![0u32; size * size])
    }

    #[test]
    fn non_zero_count_cells_are_preserved() {
        let mut h = vec![0.0f64; GRID_SIDE * GRID_SIDE];
        let mut c = vec![0u32; GRID_SIDE * GRID_SIDE];
        // Place a known height at (10, 10)
        h[10 * GRID_SIDE + 10] = 50.0; // 50 usft = 600 inches
        c[10 * GRID_SIDE + 10] = 1;

        let result = fill_gaps(&h, &c);

        assert_eq!(result[10 * GRID_SIDE + 10], 600); // 50 ft * 12
    }

    #[test]
    fn zero_count_cell_is_filled_from_neighbour() {
        let mut h = vec![0.0f64; GRID_SIDE * GRID_SIDE];
        let mut c = vec![0u32; GRID_SIDE * GRID_SIDE];
        // Surround (10, 10) with a known height at (10, 11)
        h[10 * GRID_SIDE + 11] = 100.0; // 100 usft = 1200 inches
        c[10 * GRID_SIDE + 11] = 1;
        // (10, 10) has count=0 so it should be filled from the neighbourhood

        let result = fill_gaps(&h, &c);

        assert_eq!(result[10 * GRID_SIDE + 11], 1200);
        // (10, 10) should be non-zero because its neighbour (10, 11) is 100 ft
        assert!(result[10 * GRID_SIDE + 10] > 0);
    }

    #[test]
    fn empty_neighbourhood_stays_zero() {
        let h = vec![0.0f64; GRID_SIDE * GRID_SIDE];
        let c = vec![0u32; GRID_SIDE * GRID_SIDE];
        let result = fill_gaps(&h, &c);
        assert!(result.iter().all(|&v| v == 0));
    }

    #[test]
    fn heights_converted_to_inches() {
        let mut h = vec![0.0f64; GRID_SIDE * GRID_SIDE];
        let mut c = vec![0u32; GRID_SIDE * GRID_SIDE];
        h[0] = 1.0; // 1 usft = 12 inches
        c[0] = 1;
        let result = fill_gaps(&h, &c);
        assert_eq!(result[0], 12);
    }

    #[test]
    fn height_clamped_to_u16_max() {
        let mut h = vec![0.0f64; GRID_SIDE * GRID_SIDE];
        let mut c = vec![0u32; GRID_SIDE * GRID_SIDE];
        h[0] = 100_000.0; // far above u16::MAX / 12
        c[0] = 1;
        let result = fill_gaps(&h, &c);
        assert_eq!(result[0], u16::MAX);
    }
}
