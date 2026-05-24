use std::collections::HashSet;
use derive_getters::Getters;
use derive_new::new;
use futures_util::StreamExt;
use geo::algorithm::line_measures::Distance;
use geo::{point, Euclidean, Point};
use rocket::serde::{Deserialize, Serializer};
use serde::Serialize;
use typed_floats::tf64::PositiveFinite;
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};
use crate::analysis::fresnel_zone::{compute_fresnel_zone, FresnelZone, FresnelZonePoint};
use crate::analysis::tiles::{get_intersecting_tiles, TerrainFactory, TerrainGrid};
use crate::providers::elevation_tile_provider::{CachingElevationTileProvider, ElevationTileProvider};
use crate::types::coords::{NYSCoords2, NYSCoords3};
use crate::types::errors::AssetErr;
use crate::types::obstructions::ObstructionTypesFilter;
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::TileId;

const MIN_ANALYSIS_FREQUENCY: f64 = 1_000.;
const MAX_ANALYSIS_FREQUENCY: f64 = 200_000_000_000.;

const ALPHA_ZONE_FULL: f64 = 1.0;
const ALPHA_ZONE_INNER: f64 = 0.6;

const OCCLUSION_DISTANCE_USFT: f64 = 4.0;

#[derive(Serialize,Deserialize,SchemaWrite,SchemaRead,PartialEq)]
pub enum ResultStatus {
    Unobstructed,
    PartiallyObstructed, // alpha=1.0 blocked, alpha=0.6 clear
    Obstructed, // alpha=0.6 blocked
}

pub type IntersectionResult = StairStepGrid<PositiveFinite>;

#[derive(new,Serialize,Deserialize,SchemaWrite,SchemaRead,Getters)]
pub struct ZoneEvaluation {
    zone: FresnelZone,
    intersection: IntersectionResult,
}

#[derive(new,Serialize,Deserialize,SchemaWrite,SchemaRead,Getters)]
pub struct PointEvaluationInput {
    #[serde(rename = "point_a_nys")]
    point_a: NYSCoords3,
    #[serde(rename = "point_b_nys")]
    point_b: NYSCoords3,
    frequency_hz: f64,

    #[serde(default = "ObstructionTypesFilter::default")]
    obstruction_types: ObstructionTypesFilter,
}

#[derive(Serialize,Deserialize,SchemaWrite,SchemaRead,new,Getters)]
pub struct PointEvaluationOutput {
    id: Uuid,

    #[serde(flatten)]
    input: PointEvaluationInput,

    result: ResultStatus,
}

#[derive(new,Getters,Serialize,Deserialize,SchemaWrite,SchemaRead)]
pub struct PointEvaluationOutcome {
    output: PointEvaluationOutput,

    result_full: ZoneEvaluation,
    result_inner: ZoneEvaluation,

    tiles: HashSet<TileId>,
}

pub fn valid_analysis_frequency(frequency_hz: f64) -> bool {
    frequency_hz >= MIN_ANALYSIS_FREQUENCY && frequency_hz <= MAX_ANALYSIS_FREQUENCY
}

pub async fn evaluate_points(eval_input: PointEvaluationInput, tile_provider: &(dyn ElevationTileProvider + Send + Sync)) -> Result<PointEvaluationOutcome, AssetErr> {
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

    let terrain_full = terrain_factory.load_terrain_grid(&tile_ids, &zone_full).await?;
    let terrain_inner = terrain_factory.load_terrain_grid(&tile_ids, &zone_inner).await?;

    let intersect_fn = |base_offset: &NYSCoords2| {
        let base_offset = base_offset.clone();

        move |zone_point: &FresnelZonePoint, terrain: &u16, coords: (usize, usize)| {
            intersect_inner(&endpoints, &base_offset, zone_point, terrain, coords)
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

    Ok(PointEvaluationOutcome {
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
    })
}
fn intersect_inner(
    endpoints: &(Point, Point),
    base_offset: &NYSCoords2,
    zone_point: &FresnelZonePoint,
    terrain: &u16,
    coords: (usize, usize)
) -> PositiveFinite {
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

impl PointEvaluationOutcome {
    pub fn into_output(self) -> PointEvaluationOutput { self.output }
}

#[cfg(test)]
mod tests {
    use geo::point;
    use ndarray::Array2;
    use super::*;
    use crate::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
    use crate::types::coords::{GPSCoords3, NYSCoords3};
    use crate::util::coord_conversion::CoordinateConverter;

    fn gps_to_nys(lat: f64, lon: f64, alt_m: f64) -> NYSCoords3 {
        CoordinateConverter::new().to_nys_plane3(&GPSCoords3::new(lat, lon, alt_m))
    }

    fn make_input(pa: NYSCoords3, pb: NYSCoords3, freq: f64) -> PointEvaluationInput {
        PointEvaluationInput::new(pa, pb, freq, ObstructionTypesFilter::default())
    }

    struct FlatTileProvider { elevation_inches: u16 }

    #[async_trait]
    impl ElevationTileProvider for FlatTileProvider {
        async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
            let side = usize::from(crate::types::tiles::SUBGRID_TILE_SIDE_LENGTH_USFT);
            Ok(ElevationTile::new(tile_id, Array2::from_elem((side, side), self.elevation_inches)))
        }
    }

    // --- evaluate_points ---

    #[tokio::test]
    async fn evaluate_points_flat_zero_terrain_is_unobstructed() {
        // Two antennas at the same height with flat zero terrain — the Fresnel zone
        // sits above the ground, so there should be no obstruction.
        let provider = FlatTileProvider { elevation_inches: 0 };
        let input = make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.705, -73.950, 30.0),
            5_000_000_000.0,
        );
        let outcome = evaluate_points(input, &provider).await.unwrap();
        assert!(matches!(outcome.output().result(), ResultStatus::Unobstructed));
    }

    #[tokio::test]
    async fn evaluate_points_max_terrain_is_obstructed() {
        // Terrain at u16::MAX completely fills the Fresnel zone — both the full and
        // inner zones are blocked, so the result must be Obstructed.
        let provider = FlatTileProvider { elevation_inches: u16::MAX };
        let input = make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.705, -73.950, 30.0),
            5_000_000_000.0,
        );
        let outcome = evaluate_points(input, &provider).await.unwrap();
        assert!(matches!(outcome.output().result(), ResultStatus::Obstructed));
    }

    // --- valid_analysis_frequency ---

    #[test]
    fn frequency_below_min_is_invalid() {
        assert!(!valid_analysis_frequency(MIN_ANALYSIS_FREQUENCY - 1.));
    }

    #[test]
    fn frequency_at_min_is_valid() {
        assert!(valid_analysis_frequency(MIN_ANALYSIS_FREQUENCY));
    }

    #[test]
    fn frequency_at_max_is_valid() {
        assert!(valid_analysis_frequency(MAX_ANALYSIS_FREQUENCY));
    }

    #[test]
    fn frequency_above_max_is_invalid() {
        assert!(!valid_analysis_frequency(MAX_ANALYSIS_FREQUENCY + 1.));
    }

    #[test]
    fn frequency_typical_value_is_valid() {
        assert!(valid_analysis_frequency(2_400_000_000.)); // 2.4 GHz
    }

    // --- intersect_inner ---

    fn far_endpoints() -> (geo::Point, geo::Point) {
        (point!(x: -10000.0, y: -10000.0), point!(x: 10000.0, y: 10000.0))
    }

    fn zero_offset() -> NYSCoords2 {
        NYSCoords2::new(0.0, 0.0)
    }

    #[test]
    fn intersect_terrain_above_top_returns_full_occlusion() {
        let zone_point = FresnelZonePoint::new(10, 20);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &25, (0, 0));
        assert_eq!(f64::from(result), 1.0);
    }

    #[test]
    fn intersect_terrain_at_top_returns_full_occlusion() {
        let zone_point = FresnelZonePoint::new(10, 20);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &20, (0, 0));
        assert_eq!(f64::from(result), 1.0);
    }

    #[test]
    fn intersect_terrain_below_bottom_returns_no_occlusion() {
        let zone_point = FresnelZonePoint::new(10, 20);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &5, (0, 0));
        assert_eq!(f64::from(result), 0.0);
    }

    #[test]
    fn intersect_terrain_at_bottom_returns_no_occlusion() {
        let zone_point = FresnelZonePoint::new(10, 20);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &10, (0, 0));
        assert_eq!(f64::from(result), 0.0);
    }

    #[test]
    fn intersect_terrain_midpoint_returns_half_occlusion() {
        // bottom=0, top=100, terrain=50 => 0.5
        let zone_point = FresnelZonePoint::new(0, 100);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &50, (0, 0));
        assert!((f64::from(result) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn intersect_terrain_partial_returns_fractional_occlusion() {
        // bottom=0, top=10, terrain=3 => 0.3
        let zone_point = FresnelZonePoint::new(0, 10);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &3, (0, 0));
        assert!((f64::from(result) - 0.3).abs() < 1e-10);
    }

    #[test]
    fn intersect_zero_height_zone_returns_full_occlusion() {
        // top == bottom and terrain is between them — degenerate zone
        let zone_point = FresnelZonePoint::new(10, 10);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &11, (0, 0));
        assert_eq!(f64::from(result), 1.0);
    }

    #[test]
    fn intersect_zero_height_zone_returns_zero_occlusion() {
        // top == bottom and terrain is between them — degenerate zone
        let zone_point = FresnelZonePoint::new(10, 10);
        let result = intersect_inner(&far_endpoints(), &zero_offset(), &zone_point, &5, (0, 0));
        assert_eq!(f64::from(result), 0.0);
    }

    #[test]
    fn intersect_near_endpoint_a_returns_zero() {
        // sample point at (0,0), endpoint_a at (1,1) — distance ~1.4 < OCCLUSION_DISTANCE_USFT
        let endpoints = (point!(x: 1.0, y: 1.0), point!(x: 10000.0, y: 10000.0));
        let zone_point = FresnelZonePoint::new(0, 10);
        let result = intersect_inner(&endpoints, &zero_offset(), &zone_point, &9, (0, 0));
        assert_eq!(f64::from(result), 0.0);
    }

    #[test]
    fn intersect_near_endpoint_b_returns_zero() {
        // sample point at (0,0), endpoint_b at (0,0) — distance 0 < OCCLUSION_DISTANCE_USFT
        let endpoints = (point!(x: 10000.0, y: 10000.0), point!(x: 0.0, y: 0.0));
        let zone_point = FresnelZonePoint::new(0, 10);
        let result = intersect_inner(&endpoints, &zero_offset(), &zone_point, &9, (0, 0));
        assert_eq!(f64::from(result), 0.0);
    }

    #[test]
    fn intersect_far_from_both_endpoints_uses_terrain_value() {
        // sample point at (1000, 1000), far from both endpoints
        let endpoints = (point!(x: 0.0, y: 0.0), point!(x: 0.0, y: 0.0));
        let zone_point = FresnelZonePoint::new(0, 10);
        let result = intersect_inner(&endpoints, &zero_offset(), &zone_point, &5, (1000, 1000));
        assert!((f64::from(result) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn intersect_base_offset_shifts_sample_point() {
        let endpoints = (point!(x: 0.0, y: 0.0), point!(x: 1000.0, y: 1000.0));
        let base_offset = NYSCoords2::new(998.0, 998.0);
        let zone_point = FresnelZonePoint::new(0, 10);
        let result = intersect_inner(&endpoints, &base_offset, &zone_point, &5, (2, 2));
        assert_eq!(f64::from(result), 0.0);
    }
}