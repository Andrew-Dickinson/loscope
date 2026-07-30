use crate::analysis::fresnel_zone::{fresnel_semi_axes, fresnel_zone_dims};
use crate::analysis::point_evaluation::PointEvaluationInput;
use crate::types::tiles::SUBGRID_TILE_SIDE_LENGTH_USFT;
use crate::util::env::{
    LOS_MEMORY_ESTIMATE_SAFETY_FACTOR, LOS_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE, get_env,
};

const ALPHA_ZONE_FULL: f64 = 1.0;
const ALPHA_ZONE_INNER: f64 = 0.6;

// Bytes/cell for the arrays evaluate_points keeps alive simultaneously, per zone (full or
// inner — both are computed and held at once):
//   FresnelZone value:  FresnelZonePoint (2×u16) = 4
//   TerrainGrid value:  u16                      = 2
//   IntersectionResult: FractionU8                = 1
const BYTES_PER_ZONE_CELL: u64 = 4 + 2 + 1;

const TILE_SIDE_USFT: u64 = SUBGRID_TILE_SIDE_LENGTH_USFT as u64;
const ELEVATION_TILE_BYTES: u64 = TILE_SIDE_USFT * TILE_SIDE_USFT * 2; // u16 per cell

/// Conservative per-tile allowance for obstruction rasters (building heightmaps, permit
/// footprints, etc) pulled in alongside each elevation tile. Obstruction data size is
/// data-dependent and can't be known without fetching it, so this is a fixed pad — a rough
/// guess at a modest handful of building footprints per 500×500usft tile, not a measured
/// figure. Tune via LOS_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE once real production data is
/// available; this default has already had to be revised down once after proving too high.
const DEFAULT_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE: u64 = 256 * 1024;

/// Multiplier applied to the raw estimate to cover allocator overhead, transient copies made
/// while merging grids, and general slop between the model here and observed RSS. Tune via
/// LOS_MEMORY_ESTIMATE_SAFETY_FACTOR.
const DEFAULT_SAFETY_FACTOR: f64 = 1.3;

fn obstruction_bytes_per_tile_estimate() -> u64 {
    get_env(LOS_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE)
}

fn safety_factor() -> f64 {
    get_env(LOS_MEMORY_ESTIMATE_SAFETY_FACTOR)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_SAFETY_FACTOR)
}

// ── Non-analysis endpoints ──────────────────────────────────────────────────────

// Bytes/pixel for the arrays a rooftop heightmap request keeps alive simultaneously
// (RooftopHeightMapFactory::create + the clone filter_heightmap_outliers takes internally):
//   heightmap:               u16  = 2
//   mask:                    bool = 1
//   filter_heightmap_outliers's clone of the original heightmap = 2
const BYTES_PER_HEIGHTMAP_PIXEL: u64 = 2 + 1 + 2;

/// Estimates the peak heap bytes a rooftop heightmap request (render_rooftop, sample_points)
/// will allocate, given the pixel dimensions from `heightmap_pixel_dims`. Bounded above by
/// MAX_TILES_PER_BUILDING_FOOTPRINT regardless of footprint shape.
pub fn estimate_heightmap_bytes(output_w: usize, output_h: usize) -> u64 {
    let raw = (output_w as u64) * (output_h as u64) * BYTES_PER_HEIGHTMAP_PIXEL;
    (raw as f64 * safety_factor()) as u64
}

/// get_terrain_raster and background_tile_raster each fetch exactly one fixed-size elevation
/// tile and re-encode it as TIFF: source tile + TIFF output buffer of comparable size.
pub fn elevation_tile_endpoint_bytes() -> u64 {
    ((2 * ELEVATION_TILE_BYTES) as f64 * safety_factor()) as u64
}

/// get_terrain_ortho decodes one JP2 tile (1000×1000 RGBA8 — see ortho_provider tests), then
/// applies photo adjustments and classification colorization (each producing a further
/// in-memory copy) before re-encoding as JPEG. Budget for ~4 live RGBA8-sized copies.
pub fn ortho_tile_endpoint_bytes() -> u64 {
    const ORTHO_TILE_PIXELS: u64 = 1000 * 1000;
    const ORTHO_TILE_RGBA8_BYTES: u64 = ORTHO_TILE_PIXELS * 4;
    ((4 * ORTHO_TILE_RGBA8_BYTES) as f64 * safety_factor()) as u64
}

/// Obstruction rasters (get_terrain_obstruction_obj) aren't bounded by a hard size cap the way
/// building footprints are — this is a coarse, deliberately generous flat allowance rather than
/// a computed estimate, since there's no cheap way to know an obstruction's raster size before
/// fetching it.
pub fn obstruction_obj_endpoint_bytes() -> u64 {
    const DEFAULT_OBSTRUCTION_OBJ_BYTES: u64 = 16 * 1024 * 1024;
    (DEFAULT_OBSTRUCTION_OBJ_BYTES as f64 * safety_factor()) as u64
}

/// Approximates the number of tiles `get_intersecting_tiles` will return for the full zone,
/// purely analytically — no per-row walk, just the link's distance and the ellipse's worst-case
/// cross-section width. Models the zone as a straight band of length `dist` and width
/// `2×semi_minor` crossing the tile grid diagonally, as the sum of two terms:
///   - an *area* term (band area / tile area): correct when the band is wide relative to a
///     tile — the case a naive `tiles_along × tiles_across` product is right for.
///   - a *perimeter* term (`dist/s + width/s`): the number of tiles a zero-width diagonal line
///     of this length would cross. This is what a product model gets wrong for *narrow* bands
///     (the overwhelmingly common case — most links have a cross-section well under 500usft):
///     pinning the across-dimension at its `+1` minimum and multiplying effectively doubled the
///     along-dimension's count, even though a thin diagonal line mostly stays within a single
///     tile-column for many consecutive tile-rows, only occasionally stepping into the next one.
/// Summing both terms tracks the real diagonal footprint reasonably well across both regimes,
/// and degrades gracefully (perimeter term dominates for narrow bands, area term for wide ones)
/// rather than needing a mode switch.
fn estimate_tile_count(input: &PointEvaluationInput) -> u64 {
    let pa: (f64, f64, f64) = input.point_a().into();
    let pb: (f64, f64, f64) = input.point_b().into();
    let dist = ((pb.0 - pa.0).powi(2) + (pb.1 - pa.1).powi(2) + (pb.2 - pa.2).powi(2)).sqrt();

    let (_, semi_minor) = fresnel_semi_axes(dist, *input.frequency_hz(), ALPHA_ZONE_FULL);
    let width = 2.0 * semi_minor;
    let s = TILE_SIDE_USFT as f64;

    let area_tiles = (dist * width / (s * s)).ceil();
    let perimeter_tiles = (dist / s).ceil() + (width / s).ceil();

    (area_tiles + perimeter_tiles + 1.0) as u64
}

/// Estimates the peak heap bytes `evaluate_points` will allocate for this input, without
/// running any of it. Used to admit or throttle requests before they can OOM the process.
pub fn estimate_analysis_bytes(input: &PointEvaluationInput) -> u64 {
    let (rows_full, cols_full) = fresnel_zone_dims(input, ALPHA_ZONE_FULL);
    let (rows_inner, cols_inner) = fresnel_zone_dims(input, ALPHA_ZONE_INNER);

    let zone_cells = (rows_full as u64 * cols_full as u64) + (rows_inner as u64 * cols_inner as u64);
    let zone_bytes = zone_cells * BYTES_PER_ZONE_CELL;

    // terrain_full and terrain_inner load their (identical) tile sets sequentially, not
    // concurrently (see evaluate_points) — the raw tile/obstruction data from the first load is
    // dropped before the second load starts, so only one tile-loading pass is ever resident in
    // memory at a peak moment. Not counted twice.
    let tile_count = estimate_tile_count(input);
    let tile_bytes = tile_count * (ELEVATION_TILE_BYTES + obstruction_bytes_per_tile_estimate());

    let raw_estimate = zone_bytes + tile_bytes;
    (raw_estimate as f64 * safety_factor()) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::coords::{GPSCoords3, NYSCoords3};
    use crate::types::obstructions::ObstructionTypesFilter;
    use crate::util::coord_conversion::CoordinateConverter;

    fn gps_to_nys(lat: f64, lon: f64, alt_m: f64) -> NYSCoords3 {
        CoordinateConverter::new().to_nys_plane3(&GPSCoords3::new(lat, lon, alt_m))
    }

    fn make_input(pa: NYSCoords3, pb: NYSCoords3, freq: f64) -> PointEvaluationInput {
        PointEvaluationInput::new(pa, pb, freq, ObstructionTypesFilter::All)
    }

    #[test]
    fn short_link_estimate_is_small() {
        let input = make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.705, -73.950, 30.0),
            5_000_000_000.0,
        );
        // A few-block link should be well under 100MB.
        assert!(estimate_analysis_bytes(&input) < 100 * 1024 * 1024);
    }

    #[test]
    fn long_link_estimate_exceeds_short_link() {
        let short = make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.705, -73.950, 30.0),
            5_000_000_000.0,
        );
        let long = make_input(
            gps_to_nys(40.500, -74.200, 200.0),
            gps_to_nys(41.200, -73.200, 200.0),
            5_000_000_000.0,
        );
        assert!(estimate_analysis_bytes(&long) > estimate_analysis_bytes(&short));
    }

    #[test]
    fn lower_frequency_increases_estimate_for_same_link() {
        let higher_freq = make_input(
            gps_to_nys(40.500, -74.200, 200.0),
            gps_to_nys(41.200, -73.200, 200.0),
            5_000_000_000.0,
        );
        let lower_freq = make_input(
            gps_to_nys(40.500, -74.200, 200.0),
            gps_to_nys(41.200, -73.200, 200.0),
            1_000_000.0,
        );
        assert!(estimate_analysis_bytes(&lower_freq) > estimate_analysis_bytes(&higher_freq));
    }

    #[test]
    fn moderate_link_estimate_is_reasonable() {
        // A realistic ~2-mile point-to-point link at a common ISP frequency — the shape of
        // request that's actually common in production, as opposed to the deliberately
        // pathological cases elsewhere in this file. Should land in the tens-of-MB range: this
        // is a regression guard for the tile-count overcounting bugs fixed here (axis-aligned
        // bounding box instead of the real diagonal footprint, doubling for sequential-not-
        // concurrent tile loads, and multiplicative tiles_along×tiles_across overcounting for
        // narrow bands). Note that zone_bytes (the Fresnel/terrain/intersection arrays) is
        // exact, not estimated — see fresnel_zone_dims_matches_compute_fresnel_zone — so it
        // legitimately grows for longer links; this test picks a length representative of
        // typical usage rather than asserting an arbitrary bound at any distance.
        let input = make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.718, -73.940, 30.0),
            2_400_000_000.0,
        );
        let estimate = estimate_analysis_bytes(&input);
        assert!(estimate < 100 * 1024 * 1024, "estimate was {estimate} bytes");
    }

    #[test]
    fn extreme_long_low_frequency_link_is_huge() {
        // This is the shape of request that OOMs an unthrottled server: a very long link at a
        // very low frequency. The estimate should be in the multi-gigabyte range, well above
        // any reasonable per-request memory budget.
        let input = make_input(
            gps_to_nys(40.000, -75.500, 200.0),
            gps_to_nys(42.500, -71.500, 200.0),
            1_000_000.0,
        );
        assert!(estimate_analysis_bytes(&input) > 1024 * 1024 * 1024);
    }
}
