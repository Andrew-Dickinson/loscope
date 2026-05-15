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
        assert_eq!(self.values.len(), self.widths.len());
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
                output.values[[i,j]] = merge_fn(self_val, other_val.clone(), (offset_x, offset_y));
            })
        }

        output
    }
}