use derive_getters::Getters;
use derive_new::new;
use ndarray::{s, Array1, Array2};
use rocket::serde::{Deserialize, Serialize};
use crate::types::coords::NYSCoords2;
use crate::types::tiles::{TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};

/// Sparse Array2 representation, which uses an x-offset for each row in values to shift that row
/// in the positive-x direction. The contents of row i in values are only valid up to widths[i]
#[derive(new,Serialize,Deserialize,Getters)]
pub struct StairStepGrid<T> {
    values: Array2<T>,
    widths: Array1<usize>,
    offsets: Array1<usize>,
    base_offset: NYSCoords2,
}

impl<T> StairStepGrid<T> where T: Ord{
    pub fn max(&self) -> Option<&T> {
        assert_eq!(self.values.nrows(), self.widths.len());
        self.values.rows().into_iter()
            .zip(self.widths.iter())
            .flat_map(|(row, &width)| row.into_iter().take(width))
            .max()
    }
}

impl<T> StairStepGrid<T> {
    pub fn is_empty(&self) -> bool {
        !self.widths.iter().any(|&w| w > 0)
    }

    pub fn merge<U,V: Default,F: Fn(&T,&U,(usize, usize)) -> V>(&self, other: &StairStepGrid<U>, merge_fn: F) -> StairStepGrid<V> {
        let mut output: StairStepGrid<V> = StairStepGrid {
            values: Array2::default((self.values.shape()[0], self.values.shape()[1])),
            widths: self.widths.clone(),
            offsets: self.offsets.clone(),
            base_offset: self.base_offset.clone(),
        };

        for i in 0..self.widths().len() {
            let self_row = self.values().row(i);
            let other_row = other.values().row(i);

            let width = self.widths()[i];
            assert_eq!(other.widths()[i], width);

            let offset_y = i;
            let offset_x = self.offsets()[i];
            assert_eq!(other.offsets()[i], offset_x);

            self_row.iter().zip(other_row.iter()).enumerate().for_each(|(j, (self_val, other_val))| {
                output.values[[i,j]] = merge_fn(self_val, other_val, (offset_x, offset_y));
            })
        }

        output
    }
}

impl<T> StairStepGrid<T> where T: Default + Clone {
    pub fn rasterize_in_tile(&self, tile_id: TileId) -> Array2<T> {
        let mut output = Array2::default((
                SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
                SUBGRID_TILE_SIDE_LENGTH_USFT.into()
            )
        );

        let zone_base_offset = &self.base_offset;
        let tile_base_offset  = tile_id.get_sw_corner();

        let zone_base_offset = (zone_base_offset.easting().floor() as usize, zone_base_offset.northing().floor() as usize);
        let tile_base_offset = (tile_base_offset.easting().floor() as usize, tile_base_offset.northing().floor() as usize);

        let i_start = (tile_base_offset.1 as isize - zone_base_offset.1 as isize).max(0) as usize;
        let i_end: Result<usize, _> = ((tile_base_offset.1 as isize - zone_base_offset.1 as isize) + SUBGRID_TILE_SIDE_LENGTH_USFT as isize)
            .min(self.widths.len() as isize)
            .try_into();

        let Ok(i_end) = i_end else {
            // If zone_base_offset.1 > (tile_base_offset.1 + SUBGRID_TILE_SIDE_LENGTH_USFT), the
            // tile doesn't overlap at all with the stairstep and i_end < 0 <= i_start, so we return
            // default values for the entire output raster
            return output;
        };

        for i in i_start..i_end {
            let width = self.widths[i];
            if width == 0 { continue; }

            // Safety: strict_sub() won't panic here because as constructed above,
            // min(i) = tile_base_offset.1 - zone_base_offset.1
            let tile_y = (zone_base_offset.1 + i).strict_sub(tile_base_offset.1);

            let row_start = zone_base_offset.0 + self.offsets[i];
            let row_end = row_start + width;

            let overlap_start = row_start.max(tile_base_offset.0);
            let overlap_end = row_end.min(tile_base_offset.0 + usize::from(SUBGRID_TILE_SIDE_LENGTH_USFT));
            if overlap_start >= overlap_end { continue; }

            // Safety: strict_sub() won't panic here because as constructed above,
            // overlap_end > overlap_start >= row_start &&
            // overlap_end > overlap_start >= tile_base_offset.0
            let j_start = overlap_start.strict_sub(row_start);
            let j_end = overlap_end.strict_sub(row_start);
            let tile_x_start = overlap_start.strict_sub(tile_base_offset.0);
            let tile_x_end = overlap_end.strict_sub(tile_base_offset.0);

            output.slice_mut(s![tile_x_start..tile_x_end, tile_y])
                .assign(&*self.values().slice(s![i, j_start..j_end]));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use ndarray::{array, Array1, Array2};
    use crate::types::coords::NYSCoords2;
    use crate::types::tiles::TileId;
    use super::StairStepGrid;

    fn make_grid<T: Clone>(
        values: Array2<T>,
        widths: Array1<usize>,
        offsets: Array1<usize>,
    ) -> StairStepGrid<T> {
        StairStepGrid::new(values, widths, offsets, NYSCoords2::new(0.0, 0.0))
    }

    fn make_grid_at<T: Clone>(
        values: Array2<T>,
        widths: Array1<usize>,
        offsets: Array1<usize>,
        base: (f64, f64),
    ) -> StairStepGrid<T> {
        StairStepGrid::new(values, widths, offsets, NYSCoords2::new(base.0, base.1))
    }

    // "500300_00" → SW corner (500000, 300000), NE (500500, 300500)
    fn tile() -> TileId { TileId::parse("500300_00").unwrap() }
    fn tile_sw() -> (f64, f64) { (500_000.0, 300_000.0) }

    // --- rasterize_in_tile ---

    #[test]
    fn rasterize_writes_values_at_tile_origin() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 3), vec![7u8, 8, 9]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out[[0, 0]], 7);
        assert_eq!(out[[1, 0]], 8);
        assert_eq!(out[[2, 0]], 9);
        assert_eq!(out[[3, 0]], 0); // beyond width → untouched
    }

    #[test]
    fn rasterize_multiple_rows() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((2, 2), vec![10u8, 11, 20, 21]).unwrap();
        let grid = make_grid_at(values, array![2, 2], array![0, 0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!((out[[0, 0]], out[[1, 0]]), (10, 11)); // row 0 → tile_y=0
        assert_eq!((out[[0, 1]], out[[1, 1]]), (20, 21)); // row 1 → tile_y=1
    }

    #[test]
    fn rasterize_x_offset_places_values_correctly() {
        let (e, n) = tile_sw();
        // offset=5 → data starts 5 usft east of zone base
        let values = Array2::from_shape_vec((1, 1), vec![42u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![5], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out[[5, 0]], 42);
        assert_eq!(out[[4, 0]], 0);
        assert_eq!(out[[6, 0]], 0);
    }

    #[test]
    fn rasterize_zero_width_rows_are_skipped() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((3, 2), vec![9u8; 6]).unwrap();
        // Only middle row has non-zero width
        let grid = make_grid_at(values, array![0, 2, 0], array![0, 0, 0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out[[0, 0]], 0); // row 0 skipped
        assert_eq!(out[[0, 1]], 9); // row 1 written
        assert_eq!(out[[0, 2]], 0); // row 2 skipped
    }

    #[test]
    fn rasterize_zone_south_of_tile_returns_zeros() {
        // Zone base is far south — i_start will exceed i_end, loop never executes
        let values = Array2::from_shape_vec((1, 3), vec![99u8, 99, 99]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 200_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn rasterize_zone_north_of_tile_returns_zeros() {
        // Zone base is north of the tile — i_end will be negative, early return
        let values = Array2::from_shape_vec((1, 3), vec![99u8, 99, 99]).unwrap();
        let grid = make_grid_at(values, array![3], array![0], (500_000.0, 301_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn rasterize_zone_east_of_tile_returns_zeros() {
        // Zone data is entirely east of the tile's NE easting (500500)
        let values = Array2::from_shape_vec((1, 1), vec![99u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![0], (501_000.0, 300_000.0));
        let out = grid.rasterize_in_tile(tile());
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn rasterize_partial_x_overlap_from_west() {
        // Zone starts 100 usft west of tile; 200-wide data → first 100 usft are clipped
        let (_, n) = tile_sw();
        let data: Vec<u8> = (0..200).map(|i| i as u8).collect();
        let values = Array2::from_shape_vec((1, 200), data).unwrap();
        let grid = make_grid_at(values, array![200], array![0], (499_900.0, n));
        let out = grid.rasterize_in_tile(tile());
        // j_start = 500000-499900 = 100, so out[0..100, 0] = values[100..200]
        assert_eq!(out[[0, 0]], 100);
        assert_eq!(out[[99, 0]], 199);
        assert_eq!(out[[100, 0]], 0); // beyond the 200-wide zone data
    }

    #[test]
    fn rasterize_output_is_always_tile_sized() {
        let (e, n) = tile_sw();
        let values = Array2::from_shape_vec((1, 1), vec![1u8]).unwrap();
        let grid = make_grid_at(values, array![1], array![0], (e, n));
        let out = grid.rasterize_in_tile(tile());
        assert_eq!(out.shape(), &[500, 500]);
    }

    // --- is_empty ---

    #[test]
    fn is_empty_all_zero_widths() {
        let grid = make_grid(Array2::<i32>::zeros((3, 4)), array![0, 0, 0], array![0, 0, 0]);
        assert!(grid.is_empty());
    }

    #[test]
    fn is_empty_one_nonzero_width() {
        let grid = make_grid(Array2::<i32>::zeros((3, 4)), array![0, 2, 0], array![0, 0, 0]);
        assert!(!grid.is_empty());
    }

    // --- max ---

    #[test]
    fn max_returns_none_when_empty() {
        let grid = make_grid(Array2::<i32>::zeros((2, 3)), array![0, 0], array![0, 0]);
        assert_eq!(grid.max(), None);
    }

    #[test]
    fn max_ignores_cells_beyond_width() {
        // Row 0 has width 2; columns 2+ should be ignored even if they contain large values.
        let values = Array2::from_shape_vec((1, 4), vec![1, 3, 999, 999]).unwrap();
        let grid = make_grid(values, array![2], array![0]);
        assert_eq!(grid.max(), Some(&3));
    }

    #[test]
    fn max_across_multiple_rows() {
        let values = Array2::from_shape_vec((2, 3), vec![1, 2, 3, 4, 5, 6]).unwrap();
        // Row 0 valid up to width 2 (values 1,2), row 1 valid up to width 3 (values 4,5,6)
        let grid = make_grid(values, array![2, 3], array![0, 0]);
        assert_eq!(grid.max(), Some(&6));
    }

    // --- merge ---

    #[test]
    fn merge_applies_fn_to_each_cell() {
        let a = Array2::from_shape_vec((2, 2), vec![1, 2, 3, 4]).unwrap();
        let b = Array2::from_shape_vec((2, 2), vec![10, 20, 30, 40]).unwrap();
        let g1 = make_grid(a, array![2, 2], array![0, 0]);
        let g2 = make_grid(b, array![2, 2], array![0, 0]);

        let merged = g1.merge(&g2, |x, y, _| x + y);

        assert_eq!(merged.values()[[0, 0]], 11);
        assert_eq!(merged.values()[[0, 1]], 22);
        assert_eq!(merged.values()[[1, 0]], 33);
        assert_eq!(merged.values()[[1, 1]], 44);
    }

    #[test]
    fn merge_passes_correct_offset_coords() {
        use std::cell::Cell;
        let a = Array2::from_shape_vec((1, 1), vec![0]).unwrap();
        let b = Array2::from_shape_vec((1, 1), vec![0]).unwrap();
        let g1 = make_grid(a, array![1], array![7]);
        let g2 = make_grid(b, array![1], array![7]);

        let captured = Cell::new((0usize, 0usize));
        g1.merge(&g2, |_, _, coords| { captured.set(coords); 0 });

        // offset_x comes from offsets[0]=7, offset_y is row index 0
        assert_eq!(captured.get(), (7, 0));
    }

    #[test]
    fn merge_preserves_widths_and_offsets() {
        let a = Array2::<i32>::zeros((2, 3));
        let b = Array2::<i32>::zeros((2, 3));
        let g1 = make_grid(a, array![1, 2], array![3, 4]);
        let g2 = make_grid(b, array![1, 2], array![3, 4]);

        let merged = g1.merge(&g2, |x, y, _| x + y);

        assert_eq!(merged.widths(), g1.widths());
        assert_eq!(merged.offsets(), g1.offsets());
    }
}
