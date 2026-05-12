use ndarray::{Array2};
use crate::types::coords::{NYSCoords2};


/// Sparse Array2 representation, which uses an x-offset for each row in values to shift that row
/// in the positive-x direction. The contents of row i in values are only valid up to widths[i]
pub struct StairStepGrid<T> {
    values: Array2<T>,
    widths: Vec<usize>,
    offsets: Vec<usize>,
}

