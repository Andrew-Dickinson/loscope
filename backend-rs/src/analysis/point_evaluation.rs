use derive_getters::Getters;
use derive_new::new;
use rocket::http::Status;
use rocket::serde::{Deserialize, Serializer};
use serde::Serialize;
use uuid::Uuid;
use crate::analysis::fresnel_zone::{FresnelZone, FresnelZonePoint};
use crate::types::coords::{NYSCoords2, NYSCoords3};
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::TileId;

const MIN_ANALYSIS_FREQUENCY: u64 = 1_000;
const MAX_ANALYSIS_FREQUENCY: u64 = 200_000_000_000;

#[derive(Serialize,Deserialize)]
pub enum ResultStatus {
    Unobstructed,
    PartiallyObstructed, // alpha=1.0 blocked, alpha=0.6 clear
    Obstructed, // alpha=0.6 blocked
}

#[derive(Serialize,Deserialize)]
pub enum ObstructionTypes {
    All,
    Specific(Vec<Status>)
}

impl Default for ObstructionTypes {
    fn default() -> ObstructionTypes {
        ObstructionTypes::All
    }
}

pub type IntersectionResult = StairStepGrid<FresnelZonePoint>;

#[derive(Serialize,Deserialize)]
pub struct ZoneEvaluation {
    zone: FresnelZone,
    intersection: IntersectionResult,
    max_obstruction: f64,
}

#[derive(Serialize,Deserialize,Getters)]
pub struct PointEvaluationInput {
    point_a: NYSCoords3,
    point_b: NYSCoords3,
    frequency_hz: u64,

    #[serde(default = "ObstructionTypes::default")]
    obstruction_types: ObstructionTypes,
}

#[derive(Serialize,Deserialize,new,Getters)]
pub struct PointEvaluationOutput {
    id: Uuid,

    #[serde(flatten)]
    input: PointEvaluationInput,

    result: ResultStatus,
}

#[derive(Getters,Serialize,Deserialize)]
pub struct PointEvaluationResult {
    output: PointEvaluationOutput,

    result_full: ZoneEvaluation,
    result_inner: ZoneEvaluation,

    base_offset: NYSCoords2,
    tiles: Vec<TileId>,
}

pub fn valid_analysis_frequency(frequency_hz: u64) -> bool {
    frequency_hz >= MIN_ANALYSIS_FREQUENCY && frequency_hz <= MAX_ANALYSIS_FREQUENCY
}

pub fn evaluate_points(eval_input: PointEvaluationInput) -> PointEvaluationResult {
    todo!()
}

impl PointEvaluationResult {
    pub fn into_output(self) -> PointEvaluationOutput { self.output }
}