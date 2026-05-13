use derive_getters::Getters;
use derive_new::new;
use rocket::http::Status;
use rocket::serde::{Deserialize, Serializer};
use serde::Serialize;
use uuid::Uuid;
use crate::analysis::fresnel_zone::{compute_fresnel_zone, FresnelZone, FresnelZonePoint};
use crate::analysis::tiles::{get_intersecting_tiles, TerrainFactory, TerrainGrid};
use crate::providers::elevation_tile_provider::{CachingElevationTileProvider, ElevationTileProvider};
use crate::types::coords::{NYSCoords2, NYSCoords3};
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::TileId;

const MIN_ANALYSIS_FREQUENCY: u64 = 1_000;
const MAX_ANALYSIS_FREQUENCY: u64 = 200_000_000_000;

const ALPHA_ZONE_FULL: f64 = 1.0;
const ALPHA_ZONE_INNER: f64 = 0.6;

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

pub type IntersectionResult = StairStepGrid<f64>;

#[derive(new,Serialize,Deserialize)]
pub struct ZoneEvaluation {
    zone: FresnelZone,
    intersection: IntersectionResult,
    base_offset: NYSCoords2,
}

#[derive(new,Serialize,Deserialize,Getters)]
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

#[derive(new,Getters,Serialize,Deserialize)]
pub struct PointEvaluationResult {
    output: PointEvaluationOutput,

    result_full: ZoneEvaluation,
    result_inner: ZoneEvaluation,

    tiles: Vec<TileId>,
}

pub fn valid_analysis_frequency(frequency_hz: u64) -> bool {
    frequency_hz >= MIN_ANALYSIS_FREQUENCY && frequency_hz <= MAX_ANALYSIS_FREQUENCY
}

pub fn evaluate_points(eval_input: PointEvaluationInput, tile_provider: &(dyn ElevationTileProvider + Send + Sync)) -> PointEvaluationResult {
    let analysis_id = Uuid::new_v4();

    let terrain_factory = TerrainFactory::new(tile_provider);

    let endpoints = (eval_input.point_a(), eval_input.point_b());

    let zone_full = compute_fresnel_zone(&eval_input, ALPHA_ZONE_FULL);
    let zone_inner = compute_fresnel_zone(&eval_input, ALPHA_ZONE_INNER);

    let tile_ids = get_intersecting_tiles(&zone_full);

    let terrain_full = terrain_factory.load_terrain_grid(&tile_ids, &zone_full);
    let terrain_inner = terrain_factory.load_terrain_grid(&tile_ids, &zone_inner);

    let intersection_full = compute_intersection(&zone_full, &terrain_full, &endpoints);
    let intersection_inner = compute_intersection(&zone_inner, &terrain_inner, &endpoints);

    let max_intersection_full = intersection_full.max();
    let max_intersection_inner = intersection_inner.max();

    let result = if max_intersection_full == 0.0 {
        ResultStatus::Unobstructed
    } else if max_intersection_inner == 0.0 {
        ResultStatus::PartiallyObstructed
    } else {
        ResultStatus::Obstructed
    };

    PointEvaluationResult {
        output: PointEvaluationOutput {
            id: analysis_id,
            input: eval_input,
            result,
        },
        result_full: ZoneEvaluation {
            zone: zone_full,
            intersection: intersection_full,
            base_offset: terrain_full.base_offset().clone(),
        },
        result_inner: ZoneEvaluation {
            zone: zone_inner,
            intersection: intersection_inner,
            base_offset: terrain_inner.base_offset().clone(),
        },
        tiles: vec![],
    }
}

impl PointEvaluationResult {
    pub fn into_output(self) -> PointEvaluationOutput { self.output }
}

pub fn compute_intersection(
    zone: &FresnelZone,
    terrain: &TerrainGrid,
    endpoints: &(&NYSCoords3, &NYSCoords3),
) -> IntersectionResult {
    todo!()
}