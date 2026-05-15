use derive_getters::Getters;
use derive_new::new;
use futures_util::StreamExt;
use geo::algorithm::line_measures::Distance;
use geo::{point, Euclidean, Point};
use rocket::http::Status;
use rocket::serde::{Deserialize, Serializer};
use serde::Serialize;
use typed_floats::tf64::PositiveFinite;
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

const OCCLUSION_DISTANCE_USFT: f64 = 4.0;

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

pub type IntersectionResult = StairStepGrid<PositiveFinite>;

#[derive(new,Serialize,Deserialize)]
pub struct ZoneEvaluation {
    zone: FresnelZone,
    intersection: IntersectionResult,
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

    let endpoints: (Point<f64>, Point<f64>) = (eval_input.point_a().into(), eval_input.point_b().into());

    let zone_full = compute_fresnel_zone(&eval_input, ALPHA_ZONE_FULL);
    let zone_inner = compute_fresnel_zone(&eval_input, ALPHA_ZONE_INNER);
    if zone_inner.is_empty() || zone_full.is_empty() {
        // degenerate case, endpoints are too close together
        return todo!()
    }

    let tile_ids = get_intersecting_tiles(&zone_full);

    let terrain_full = terrain_factory.load_terrain_grid(&tile_ids, &zone_full);
    let terrain_inner = terrain_factory.load_terrain_grid(&tile_ids, &zone_inner);

    let intersect_fn = |base_offset: &NYSCoords2| {
        let base_offset = base_offset.clone();
        move |zone_point: &FresnelZonePoint, terrain: &u16, coords: (usize, usize)| -> PositiveFinite {
            let top = zone_point.top();
            let bottom = zone_point.bottom();

            let intersection = if *terrain >= top {
                PositiveFinite::new(1.0).unwrap()
            } else if *terrain <= bottom {
                PositiveFinite::new(0.0).unwrap()
            } else {
                let height: f64 = (top - bottom).into();
                if height == 0.0 {
                    PositiveFinite::new(1.0).unwrap()
                } else {
                    assert!(height > 0.0);
                    // Safety: from above, we know terrain > bottom, so this result must be positive
                    PositiveFinite::new(f64::from(*terrain - bottom) / height).unwrap()
                }
            };

            let sample_point = point!(
                x: coords.0 as f64 + base_offset.easting(),
                y: coords.1 as f64 + base_offset.northing()
            );

            if Euclidean.distance_within(sample_point, endpoints.0, OCCLUSION_DISTANCE_USFT)
                || Euclidean.distance_within(sample_point, endpoints.1, OCCLUSION_DISTANCE_USFT)
            {
                PositiveFinite::new(0.0).unwrap()
            } else {
                intersection
            }
        }
    };

    let intersection_full = zone_full.merge(&terrain_full, intersect_fn(terrain_full.base_offset()));
    let intersection_inner = zone_inner.merge(&terrain_inner, intersect_fn(terrain_inner.base_offset()));

    // Safety: these unwraps only panic if the intersections are empty, which should only happen
    // in the degenerate case we Err-ed on above
    let max_intersection_full = intersection_full.max().unwrap();
    let max_intersection_inner = intersection_inner.max().unwrap();

    let result = if *max_intersection_full == 0.0 {
        ResultStatus::Unobstructed
    } else if *max_intersection_inner == 0.0 {
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
        },
        result_inner: ZoneEvaluation {
            zone: zone_inner,
            intersection: intersection_inner,
        },
        tiles: tile_ids,
    }
}

impl PointEvaluationResult {
    pub fn into_output(self) -> PointEvaluationOutput { self.output }
}