use derive_getters::Getters;
use derive_new::new;
use rocket::serde::{Deserialize, Serializer};
use serde::Serialize;
use uuid::Uuid;
use crate::analysis::fresnel_zone::{FresnelZone, FresnelZonePoint};
use crate::types::coords::{NYSCoords2, NYSCoords3};
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::TileId;

const MIN_ANALYSIS_FREQUENCY: u64 = 1_000;
const MAX_ANALYSIS_FREQUENCY: u64 = 200_000_000_000;

#[derive(Serialize)]
pub enum ObstructionStatus {
    Unobstructed,
    PartiallyObstructed, // alpha=1.0 blocked, alpha=0.6 clear
    Obstructed, // alpha=0.6 blocked
}

pub type IntersectionResult = StairStepGrid<FresnelZonePoint>;

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
}

#[derive(Serialize,new)]
pub struct PointEvaluationOutput {
    id: Uuid,

    #[serde(flatten)]
    input: PointEvaluationInput,

    result: ObstructionStatus,
}

#[derive(Getters)]
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

pub fn evaluate_and_store(eval_input: PointEvaluationInput) -> PointEvaluationResult {
    let result = evaluate_points(eval_input);

    // TODO: Store result for later retrieval by ID

    result
}

impl PointEvaluationResult {
    pub fn into_output(self) -> PointEvaluationOutput { self.output }
}