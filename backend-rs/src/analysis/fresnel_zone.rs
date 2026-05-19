use derive_more::From;
use rocket::serde::{Deserialize, Serialize};
use crate::analysis::point_evaluation::PointEvaluationInput;

use crate::types::stairstep::StairStepGrid;

#[derive(Serialize,Deserialize,From)]
pub struct FresnelZonePoint(u16, u16);

impl FresnelZonePoint {
    pub fn new(bottom: u16, top: u16) -> FresnelZonePoint { FresnelZonePoint(bottom, top) }
    pub fn bottom(&self) -> u16 { self.0 }
    pub fn top(&self) -> u16 { self.1 }
}

pub type FresnelZone = StairStepGrid<FresnelZonePoint>;


pub fn compute_fresnel_zone(point_evaluation_input: &PointEvaluationInput, alpha: f64) -> FresnelZone {
    todo!()
}