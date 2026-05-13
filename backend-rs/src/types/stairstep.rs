use derive_new::new;
use ndarray::{Array1, Array2};
use rocket::serde::{Deserialize, Serialize};


/// Sparse Array2 representation, which uses an x-offset for each row in values to shift that row
/// in the positive-x direction. The contents of row i in values are only valid up to widths[i]
#[derive(new,Serialize,Deserialize)]
pub struct StairStepGrid<T> {
    values: Array2<T>,
    widths: Array1<usize>,
    offsets: Array1<usize>,
}

impl<T> StairStepGrid<T> where T: PartialOrd {
    pub fn max(&self) -> T {
        todo!()
    }
}