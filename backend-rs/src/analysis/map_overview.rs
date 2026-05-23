use derive_getters::Getters;
use derive_new::new;
use rocket::serde::{Deserialize, Serialize};
use crate::analysis::point_evaluation::PointEvaluationOutcome;
use crate::types::coords::{GPSCoords2, GPSCoords3, NYSCoords2, NYSCoords3};
use crate::types::tiles::TileId;
use crate::util::coord_conversion::with_coord_converter;

#[derive(new,Serialize,Deserialize)]
pub struct TileResult {
    id: TileId,
    bounds: (GPSCoords2, GPSCoords2),
    intersection_detected: bool,
}


#[derive(new,Getters,Serialize,Deserialize)]
pub struct PointEvaluationOverview {
    endpoints: (GPSCoords3, GPSCoords3),
    tiles: Vec<TileResult>,
    overhead_ellipse_poly: Vec<GPSCoords2>
}

impl From<&PointEvaluationOutcome> for PointEvaluationOverview {
    fn from(value: &PointEvaluationOutcome) -> Self {
        with_coord_converter(
            |converter| {
                let input = value.output().input();
                PointEvaluationOverview {
                    endpoints: (
                        converter.to_gps3(input.point_a()),
                        converter.to_gps3(input.point_b())
                    ),
                    tiles: value.tiles().iter().map(|&tile_id| {
                        TileResult {
                            id: tile_id,
                            bounds: {
                                let tile_bounds = tile_id.get_bounds();
                                let (tile_w, tile_s) = tile_bounds.min().x_y();
                                let (tile_e, tile_n) = tile_bounds.max().x_y();

                                (
                                    converter.to_gps2(&NYSCoords2::new(tile_w.into(), tile_s.into())),
                                    converter.to_gps2(&NYSCoords2::new(tile_e.into(), tile_n.into()))
                                )
                            },
                            intersection_detected: value.result_full()
                                .intersection()
                                .rasterize_in_tile(tile_id)
                                .into_iter() // rows
                                .into_iter() // cols
                                .max()
                                .map(f64::from)
                                .unwrap_or(0.0)
                                 > 0.0
                        }
                    }).collect(),
                    overhead_ellipse_poly: generate_ellipse_poly(
                            input.point_a(),
                            input.point_b(),
                            *input.frequency_hz(),
                        )
                        .map(|coords| converter.to_gps2(&coords))
                        .collect()
                }
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    use ndarray::{Array1, Array2};
    use uuid::Uuid;
    use super::*;
    use crate::analysis::point_evaluation::{
        IntersectionResult, ObstructionTypes, PointEvaluationInput,
        PointEvaluationOutcome, PointEvaluationOutput, ResultStatus, ZoneEvaluation,
    };
    use crate::types::stairstep::StairStepGrid;
    use crate::types::tiles::TileId;
    use crate::util::coord_conversion::{
        init_coord_converter_factory, CoordinateConverter, with_coord_converter,
    };

    static SETUP: OnceLock<()> = OnceLock::new();

    fn setup() {
        SETUP.get_or_init(|| {
            // Guard against the factory already having been set by another test module.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                init_coord_converter_factory(|| CoordinateConverter::new());
            }));
        });
    }

    fn empty_zone() -> ZoneEvaluation {
        let zone = StairStepGrid::new(
            Array2::default((1, 1)),
            Array1::from_vec(vec![0]),
            Array1::from_vec(vec![0]),
            NYSCoords2::new(0.0, 0.0),
        );
        let intersection: IntersectionResult = StairStepGrid::new(
            Array2::default((1, 1)),
            Array1::from_vec(vec![0]),
            Array1::from_vec(vec![0]),
            NYSCoords2::new(0.0, 0.0),
        );
        ZoneEvaluation::new(zone, intersection)
    }

    fn make_outcome(
        point_a: NYSCoords3,
        point_b: NYSCoords3,
        tiles: HashSet<TileId>,
    ) -> PointEvaluationOutcome {
        let input = PointEvaluationInput::new(
            point_a, point_b, 2_400_000_000.0, ObstructionTypes::All,
        );
        let output = PointEvaluationOutput::new(Uuid::new_v4(), input, ResultStatus::Unobstructed);
        PointEvaluationOutcome::new(output, empty_zone(), empty_zone(), tiles)
    }

    #[test]
    fn endpoints_map_point_a_to_first_and_point_b_to_second() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let b = NYSCoords3::new(1_040_000.0, 176_500.0, 0.0);
        let outcome = make_outcome(a.clone(), b.clone(), HashSet::new());
        let overview = PointEvaluationOverview::from(&outcome);

        with_coord_converter(|conv| {
            let exp_a = conv.to_gps3(&a);
            let exp_b = conv.to_gps3(&b);
            assert_eq!(overview.endpoints().0.lat(), exp_a.lat());
            assert_eq!(overview.endpoints().0.lon(), exp_a.lon());
            assert_eq!(overview.endpoints().1.lat(), exp_b.lat());
            assert_eq!(overview.endpoints().1.lon(), exp_b.lon());
        });
    }

    #[test]
    fn no_tiles_gives_empty_tiles_vec() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let b = NYSCoords3::new(1_040_000.0, 176_500.0, 0.0);
        let outcome = make_outcome(a, b, HashSet::new());
        let overview = PointEvaluationOverview::from(&outcome);
        assert!(overview.tiles().is_empty());
    }

    #[test]
    fn tile_count_matches_input() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let b = NYSCoords3::new(1_040_000.0, 176_500.0, 0.0);
        let tile = TileId::parse("500300_00").unwrap();
        let outcome = make_outcome(a, b, HashSet::from([tile]));
        let overview = PointEvaluationOverview::from(&outcome);
        assert_eq!(overview.tiles().len(), 1);
    }

    #[test]
    fn intersection_detected_false_for_empty_intersection() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let b = NYSCoords3::new(1_040_000.0, 176_500.0, 0.0);
        let tile = TileId::parse("500300_00").unwrap();
        let outcome = make_outcome(a, b, HashSet::from([tile]));
        let overview = PointEvaluationOverview::from(&outcome);
        assert!(!overview.tiles()[0].intersection_detected);
    }

    #[test]
    fn ellipse_poly_has_91_points_for_normal_link() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let b = NYSCoords3::new(1_040_000.0, 176_500.0, 0.0);
        let outcome = make_outcome(a, b, HashSet::new());
        let overview = PointEvaluationOverview::from(&outcome);
        assert_eq!(overview.overhead_ellipse_poly().len(), 91);
    }

    #[test]
    fn ellipse_poly_is_empty_for_coincident_endpoints() {
        setup();
        let a = NYSCoords3::new(1_039_748.806, 176_148.995, 0.0);
        let outcome = make_outcome(a.clone(), a, HashSet::new());
        let overview = PointEvaluationOverview::from(&outcome);
        assert!(overview.overhead_ellipse_poly().is_empty());
    }
}

pub fn generate_ellipse_poly(
    nys_a: &NYSCoords3,
    nys_b: &NYSCoords3,
    frequency_hz: f64,
) -> impl Iterator<Item = NYSCoords2> {
    const C_USFT_PER_S: f64 = 299_792_458.0 / 0.3048006096;
    const N_PTS: usize = 90;

    let cx = (nys_a.easting() + nys_b.easting()) / 2.0;
    let cy = (nys_a.northing() + nys_b.northing()) / 2.0;
    let dx = nys_b.easting() - nys_a.easting();
    let dy = nys_b.northing() - nys_a.northing();
    let l = (dx * dx + dy * dy).sqrt();

    let mut pts: Vec<NYSCoords2> = Vec::new();
    if l == 0.0 {
        return pts.into_iter();
    }

    let theta = dy.atan2(dx);
    let semi_major = l / 2.0;
    let wavelength_usft = C_USFT_PER_S / frequency_hz;
    let semi_minor = (wavelength_usft * l / 4.0).sqrt();

    pts.reserve(N_PTS + 1);
    for i in 0..=N_PTS {
        let t = 2.0 * std::f64::consts::PI * i as f64 / N_PTS as f64;
        let xl = semi_major * t.cos();
        let yl = semi_minor * t.sin();
        pts.push(NYSCoords2::new(
            cx + xl * theta.cos() - yl * theta.sin(),
            cy + xl * theta.sin() + yl * theta.cos(),
        ));
    }
    pts.into_iter()
}