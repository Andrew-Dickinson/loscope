use derive_getters::Getters;
use derive_new::new;
use ndarray::{s, Array1, Array2};
use rocket::serde::{Deserialize, Serialize};
use crate::types::coords::NYSCoords2;

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
}

impl<T> StairStepGrid<T> {
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

#[cfg(test)]
mod tests {
    use ndarray::{array, Array1, Array2};
    use crate::types::coords::NYSCoords2;
    use super::StairStepGrid;

    fn make_grid<T: Clone>(
        values: Array2<T>,
        widths: Array1<usize>,
        offsets: Array1<usize>,
    ) -> StairStepGrid<T> {
        StairStepGrid::new(values, widths, offsets, NYSCoords2::new(0.0, 0.0))
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
