use derive_new::new;
use rocket::serde::{Serialize};
use crate::types::coords::{NYSCoords2, NYSCoords3, RelativeCoords3};

#[derive(Serialize, new)]
pub struct EncodedPoint {
    relative: RelativeCoords3,
    nys: NYSCoords3,
}

#[derive(Serialize, new)]
pub struct SamplePoint {
    sample_point: EncodedPoint,
    display_point: EncodedPoint,
}

#[derive(new,Serialize)]
pub struct SamplePoints {
    sample_points: Vec<SamplePoint>,
    sw_offset: NYSCoords2,
}