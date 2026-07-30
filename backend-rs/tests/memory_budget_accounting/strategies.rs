// proptest input generators covering the input domains real callers can actually hit through the
// HTTP API: arbitrary (but validly-bounded) link geometry/frequency for `/analyzePointPair`, and
// arbitrary (but tile-cap-bounded, matching `get_intersecting_tiles`'s own enforcement) building
// footprint bounding boxes for the rooftop heightmap endpoints.

use geo::{Polygon, polygon};
use loscope::analysis::point_evaluation::PointEvaluationInput;
use loscope::types::coords::NYSCoords3;
use loscope::types::obstructions::ObstructionTypesFilter;
use proptest::prelude::*;
use std::f64::consts::TAU;

/// Keeps generated coordinates well inside the valid [0, 2_000_000] NYS plane range so that
/// point_a +/- dist*trig never needs clamping (which would silently shrink the requested
/// distance and undermine control over the generated range).
const NYS_MARGIN_USFT: f64 = 5_000.0;

/// Generates `PointEvaluationInput`s with a controlled straight-line distance and frequency range,
/// mirroring exactly what `evaluate_points`/`estimate_analysis_bytes` consume. `max_dist_usft`
/// must leave `2 * (NYS_MARGIN_USFT + max_dist_usft) < 2_000_000.0` or the coordinate range
/// strategy is empty and this will panic when used.
pub fn point_pair_strategy(
    min_dist_usft: f64,
    max_dist_usft: f64,
    min_freq_hz: f64,
    max_freq_hz: f64,
) -> impl Strategy<Value = PointEvaluationInput> {
    let lo = NYS_MARGIN_USFT + max_dist_usft;
    let hi = 2_000_000.0 - NYS_MARGIN_USFT - max_dist_usft;
    assert!(lo < hi, "max_dist_usft={max_dist_usft} leaves no valid coordinate range");

    (
        lo..hi,
        lo..hi,
        -200.0f64..1000.0,
        -200.0f64..1000.0,
        min_dist_usft..max_dist_usft,
        0.0f64..TAU,
        min_freq_hz..max_freq_hz,
    )
        .prop_map(|(easting_a, northing_a, alt_a, alt_b, dist, bearing, freq)| {
            let point_a = NYSCoords3::new(easting_a, northing_a, alt_a);
            let easting_b = easting_a + dist * bearing.cos();
            let northing_b = northing_a + dist * bearing.sin();
            let point_b = NYSCoords3::new(easting_b, northing_b, alt_b);
            PointEvaluationInput::new(point_a, point_b, freq, ObstructionTypesFilter::All)
        })
}

/// Generates (width_usft, height_usft) footprint bounding-box dimensions whose implied tile count
/// (ceil(w/500) * ceil(h/500)) never exceeds `MAX_TILES_PER_BUILDING_FOOTPRINT` (500) -- the same
/// cap `building::heightmap::get_intersecting_tiles` enforces in production, so every case this
/// generates is one `RooftopHeightMapFactory::create` will actually accept rather than reject.
pub fn footprint_bounds_strategy() -> impl Strategy<Value = (f64, f64)> {
    (1u32..=500u32)
        .prop_flat_map(|tiles_w| {
            let max_tiles_h = 500 / tiles_w;
            (Just(tiles_w), 1u32..=max_tiles_h)
        })
        .prop_flat_map(|(tiles_w, tiles_h)| {
            let max_w = f64::from(tiles_w) * 500.0;
            let max_h = f64::from(tiles_h) * 500.0;
            let min_w = if tiles_w > 1 { f64::from(tiles_w - 1) * 500.0 + 1.0 } else { 1.0 };
            let min_h = if tiles_h > 1 { f64::from(tiles_h - 1) * 500.0 + 1.0 } else { 1.0 };
            (min_w..=max_w, min_h..=max_h)
        })
}

/// Generates point pairs whose bearing is deliberately close to due-east/due-west.
///
/// Why this exists: `AngleContext::from_delta` (analysis/angle_context.rs) computes
/// `tan_theta = -dy/dx`, and `fresnel_zone_dims`/`compute_fresnel_zone` (analysis/fresnel_zone.rs)
/// both size their grid via `max_width = (2.0 * semi_minor / sin_theta).ceil() + 1`. For a link
/// whose north/south displacement (dy) is small relative to its east/west displacement (dx),
/// `sin_theta -> 0` and `max_width` blows up -- *not* a probability-zero edge case someone would
/// need to hit exactly: it degrades smoothly as bearing approaches due-east/west, so any
/// predominantly-east-west link (not contrived at all -- e.g. a link that runs along an
/// east-west street) pays a real, severe memory cost that a similarly-long diagonal or
/// north-south link would not. At bearing *exactly* 0/pi (dy == 0.0 exactly) this becomes a
/// division by zero and the surrounding integer-grid math panics -- see
/// `exact_due_east_link_panics_in_fresnel_geometry_not_a_memory_bug` below, which documents that
/// distinct (non-memory-accounting) crash bug found incidentally while building this suite.
///
/// This is used to check that `estimate_analysis_bytes`'s claim to be "exact" (not a heuristic)
/// for the zone-sizing term actually holds in this pathological corner too -- i.e. that the budget
/// correctly throttles these requests rather than under-counting them and letting them through.
pub fn near_east_west_point_pair_strategy(
    dist_usft: f64,
    min_freq_hz: f64,
    max_freq_hz: f64,
) -> impl Strategy<Value = PointEvaluationInput> {
    let lo = NYS_MARGIN_USFT + dist_usft;
    let hi = 2_000_000.0 - NYS_MARGIN_USFT - dist_usft;
    assert!(lo < hi, "dist_usft={dist_usft} leaves no valid coordinate range");

    // Bearing offset from due-east, radians. Deliberately excludes exactly 0.0/PI (that's the
    // separate panic case) but gets close enough on both sides, and both the east- and
    // west-facing case, to exercise the 1/sin_theta blowup.
    let bearing = prop_oneof![
        0.0001f64..0.02,
        (-0.02f64)..(-0.0001),
        (std::f64::consts::PI + 0.0001)..(std::f64::consts::PI + 0.02),
    ];

    (lo..hi, lo..hi, -200.0f64..1000.0, -200.0f64..1000.0, bearing, min_freq_hz..max_freq_hz).prop_map(
        move |(easting_a, northing_a, alt_a, alt_b, bearing, freq)| {
            let point_a = NYSCoords3::new(easting_a, northing_a, alt_a);
            let easting_b = easting_a + dist_usft * bearing.cos();
            let northing_b = northing_a + dist_usft * bearing.sin();
            let point_b = NYSCoords3::new(easting_b, northing_b, alt_b);
            PointEvaluationInput::new(point_a, point_b, freq, ObstructionTypesFilter::All)
        },
    )
}

/// A simple axis-aligned rectangle footprint anchored at (base_easting, base_northing), sized
/// width_usft x height_usft.
pub fn rect_footprint(base_easting: f64, base_northing: f64, width_usft: f64, height_usft: f64) -> Polygon {
    polygon![
        (x: base_easting, y: base_northing),
        (x: base_easting + width_usft, y: base_northing),
        (x: base_easting + width_usft, y: base_northing + height_usft),
        (x: base_easting, y: base_northing + height_usft),
        (x: base_easting, y: base_northing),
    ]
}
