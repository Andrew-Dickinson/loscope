use crate::analysis::fresnel_zone::{
    compute_fresnel_zone_footprint, fresnel_semi_axes, fresnel_zone_dims,
};
use crate::analysis::point_evaluation::PointEvaluationInput;
use crate::providers::obstruction_provider::ObstructionProvider;
use crate::types::errors::AssetErr;
use crate::types::obstructions::{ObstructionId, ObstructionType};
use crate::types::tiles::SUBGRID_TILE_SIDE_LENGTH_USFT;
use crate::util::env::{LOS_MEMORY_ESTIMATE_SAFETY_FACTOR, get_env, LOS_OBSTRUCTION_BYTES_ESTIMATE};
use std::collections::HashSet;

const ALPHA_ZONE_FULL: f64 = 1.0;
const ALPHA_ZONE_INNER: f64 = 0.6;

// Bytes/cell for the arrays evaluate_points keeps alive simultaneously, per zone (full or
// inner — both are computed and held at once):
//   FresnelZone value:  FresnelZonePoint (2×u16) = 4
//   TerrainGrid value:  u16                      = 2
//   IntersectionResult: FractionU8                = 1
const BYTES_PER_ZONE_CELL: u64 = 4 + 2 + 1;

// Bytes/cell still resident in the *returned* PointEvaluationOutcomeFull once evaluate_points has
// finished: the TerrainGrid working array above is a transient local, already dropped by the time
// the function returns, unlike the FresnelZone/IntersectionResult pair that make up the returned
// ZoneEvaluations. Used by estimate_analysis_result_bytes, the shrink target for a reservation
// made via estimate_analysis_bytes_precise.
const BYTES_PER_ZONE_CELL_RESULT: u64 = 4 + 1;

const TILE_SIDE_USFT: u64 = SUBGRID_TILE_SIDE_LENGTH_USFT as u64;
const ELEVATION_TILE_BYTES: u64 = TILE_SIDE_USFT * TILE_SIDE_USFT * 2; // u16 per cell

/// Bytes budgeted per *individual* obstruction (not per tile) once its real raster is fetched --
/// covers the ObstructionRaster (u16/px) held alongside its ObstructionMeta in
/// TerrainFactory::load_terrain_grid's `obstructions: Vec`. Real obstructions
/// are all <=1 MB -- this covers that worst case outright, with headroom,
/// rather than just the realistic common case. The extra memory will be released when the actual
/// file is loaded, so this over-approximation isn't that inefficient
const DEFAULT_PER_OBSTRUCTION_BYTES_ESTIMATE: u64 = 1300 * 1024; // ~1.3 MiB

/// Multiplier applied to the raw estimate to cover allocator overhead, transient copies made
/// while merging grids, and general slop between the model here and observed RSS. Tune via
/// LOS_MEMORY_ESTIMATE_SAFETY_FACTOR.
const DEFAULT_SAFETY_FACTOR: f64 = 1.5;

fn obstruction_bytes_estimate() -> u64 {
    get_env(LOS_OBSTRUCTION_BYTES_ESTIMATE)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PER_OBSTRUCTION_BYTES_ESTIMATE)
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
/// in-memory copy) before re-encoding as JPEG. Budget for ~4 live RGBA8-sized copies, plus an
/// explicit allowance for `CachingOrthoProvider::get_ortho`'s raw-source-file buffer
/// (`Vec::with_capacity(asset_size)` + `read_to_end` over the *whole* backing JP2 — which covers
/// a full LAS-tile-sized source image, not just the requested 1000x1000 region, per its
/// `ORTHO_IMAGE_SIZE_PIXELS: u16 = 5000` sizing — in ortho_provider.rs).
///
/// `ORTHO_SOURCE_RAW_BUFFER_BYTES` (20MiB) is a flat constant, not a computed figure, because the
/// asset's real on-disk size isn't known until after `try_reserve` would need to run (getting it
/// first would mean an extra metadata round-trip before every reservation) — but unlike obstruction
/// rasters, ortho JP2 files are a fixed-format/fixed-resolution pipeline output, not something a
/// request can influence, so a flat constant calibrated with headroom over the real dataset is
/// appropriate here (the real fixture this is tested against, tests/resources/002205.jp2, is
/// ~9.6MB — see `ortho_raw_buffer_breakeven_point_against_endpoint_budget` in
/// tests/memory_budget_accounting.rs, which now regression-guards this margin directly instead of
/// relying on undocumented safety-factor slack to absorb it).
pub fn ortho_tile_endpoint_bytes() -> u64 {
    const ORTHO_TILE_PIXELS: u64 = 1000 * 1000;
    const ORTHO_TILE_RGBA8_BYTES: u64 = ORTHO_TILE_PIXELS * 4;
    const ORTHO_SOURCE_RAW_BUFFER_BYTES: u64 = 20 * 1024 * 1024;
    ((4 * ORTHO_TILE_RGBA8_BYTES + ORTHO_SOURCE_RAW_BUFFER_BYTES) as f64 * safety_factor()) as u64
}

/// Obstruction rasters (get_terrain_obstruction_obj) aren't bounded by a hard size cap the way
/// building footprints are — this is a coarse, deliberately generous flat allowance rather than
/// a computed estimate, since there's no cheap way to know an obstruction's raster size before
/// fetching it.
pub fn obstruction_obj_endpoint_bytes() -> u64 {
    const DEFAULT_OBSTRUCTION_OBJ_BYTES: u64 = 16 * 1024 * 1024;
    (DEFAULT_OBSTRUCTION_OBJ_BYTES as f64 * safety_factor()) as u64
}

/// `intersection_visualization` (endpoints/analysis.rs) rasterizes one tile's intersection result
/// into a fixed OUT_SIDE x OUT_SIDE (4000x4000) RGBA8 image (see analysis/intersection_vis.rs)
/// and re-encodes it as PNG. Both the raw RGBA buffer and the PNG encoder's internal zlib/scanline
/// buffers scale with this same fixed pixel count, not with request input, so this is a flat,
/// non-data-dependent allowance covering "the raw buffer + ~1 comparable-sized copy for
/// PNG-encoding overhead" — see `intersection_visualization_png_bytes_covers_measured_allocation`
/// in tests/memory_budget_accounting.rs for the measured figure this was calibrated against.
///
/// This does *not* cover the `get_full` recompute this endpoint may also trigger — see
/// `estimate_full_recompute_bytes`, which callers must additionally reserve for.
pub fn intersection_visualization_png_bytes() -> u64 {
    const OUT_SIDE: u64 = 4000;
    const RGBA_BYTES_PER_PIXEL: u64 = 4;
    const RAW_AND_ENCODE_OVERHEAD_MULTIPLIER: u64 = 2;
    ((OUT_SIDE * OUT_SIDE * RGBA_BYTES_PER_PIXEL * RAW_AND_ENCODE_OVERHEAD_MULTIPLIER) as f64
        * safety_factor()) as u64
}

/// `overview`, `intersectionVisualization`, `fresnelSliceObj`, and `fresnelKml` (endpoints/
/// analysis.rs) all call `PointEvaluationResultProvider::get_full`, which — whenever the cached
/// Full result has expired (30s TTL) but the Lite result is still cached (3h TTL) — transparently
/// recomputes it via `PointEvaluationOutcomeLite::to_full` calling `evaluate_points`. That's
/// exactly the computation `estimate_analysis_bytes_precise` already models, so this is a thin,
/// intention-documenting wrapper around it for that call site rather than a separate formula. All
/// four endpoints now reserve via this before calling `get_full` and shrink to
/// `estimate_analysis_result_bytes` once it returns, the same pattern `point_analysis` uses for
/// the initial `evaluate_points` call.
pub async fn estimate_full_recompute_bytes(
    input: &PointEvaluationInput,
    obstruction_provider: &(dyn ObstructionProvider + Send + Sync),
) -> Result<u64, AssetErr> {
    estimate_analysis_bytes_precise(input, obstruction_provider).await
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

/// A more accurate version of `estimate_analysis_bytes` that replaces its flat
/// `tile_count * obstruction_bytes_per_tile_estimate()` guess with the *real* per-tile obstruction
/// count, obtained from `obstruction_provider` — for the production `CachingObstructionProvider`
/// this is a cheap, synchronous, in-memory index lookup (the whole index is loaded once at
/// startup precisely so this kind of query is cheap), not a network fetch, so doing this before
/// admission is inexpensive relative to the terrain-loading work it's protecting against. This
/// also uses the *real* tile set (via `get_intersecting_tiles` on the actual computed zone
/// footprint) rather than `estimate_tile_count`'s analytical approximation, so both the
/// elevation-tile and obstruction components of the estimate are exact rather than approximated.
///
/// This does re-run the zone geometry computation once — the same work `evaluate_points` will do
/// again internally — to get a real tile set to query, via `compute_fresnel_zone_footprint`
/// rather than the full `compute_fresnel_zone`: the footprint variant computes `widths`/`offsets`
/// (all `get_intersecting_tiles` needs) without allocating the full `values` grid, which is
/// exactly `zone_bytes`-sized. Calling the full `compute_fresnel_zone` here instead was a real bug
/// caught by profiling this function under real concurrent load (see
/// `util::memory_profiler`/`docs` — real RSS spiked into multiple GB, far past the configured
/// budget, while `reserved_bytes` stayed correctly capped): every call to this estimator —
/// including for requests that go on to be rejected — was unconditionally paying the zone's full
/// peak allocation cost *before* any reservation existed to account for it, which is precisely the
/// failure mode the memory budget exists to prevent. Using the footprint variant closes that gap
/// while keeping this function's actual job (an exact tile count for the obstruction query below)
/// unchanged.
///
/// Pair with `estimate_analysis_result_bytes` and `Reservation::shrink_to` once the real
/// `evaluate_points`/`to_full` call returns: this estimate covers the *peak* (while terrain and
/// obstruction rasters are loaded), which is much larger than what's still resident afterward.
pub async fn estimate_analysis_bytes_precise(
    input: &PointEvaluationInput,
    obstruction_provider: &(dyn ObstructionProvider + Send + Sync),
) -> Result<u64, AssetErr> {
    let (rows_full, cols_full) = fresnel_zone_dims(input, ALPHA_ZONE_FULL);
    let (rows_inner, cols_inner) = fresnel_zone_dims(input, ALPHA_ZONE_INNER);
    let zone_cells = (rows_full as u64 * cols_full as u64) + (rows_inner as u64 * cols_inner as u64);
    let zone_bytes = zone_cells * BYTES_PER_ZONE_CELL;

    let (widths, offsets, base_offset) = compute_fresnel_zone_footprint(input, ALPHA_ZONE_FULL);
    let tile_ids = crate::analysis::tiles::get_intersecting_tiles(&widths, &offsets, &base_offset);
    let tile_count = tile_ids.len() as u64;

    // Dedupe the same way TerrainFactory::load_terrain_grid does (its `all_obstruction_ids:
    // HashSet<(ObstructionType, ObstructionId)>`): an obstruction whose footprint spans multiple
    // tiles is listed under each tile's index entry but fetched/held only once.
    let mut distinct_obstructions: HashSet<(ObstructionType, ObstructionId)> = HashSet::new();
    for tile_id in &tile_ids {
        let ids_by_type = obstruction_provider.get_obstruction_ids_for_tile(*tile_id).await?;
        for (obs_type, ids) in ids_by_type {
            if !input.obstruction_types().includes(&obs_type) {
                continue;
            }
            distinct_obstructions.extend(ids.into_iter().map(|id| (obs_type.clone(), id)));
        }
    }
    let obstruction_bytes = distinct_obstructions.len() as u64 * obstruction_bytes_estimate();

    let tile_bytes = tile_count * ELEVATION_TILE_BYTES + obstruction_bytes;
    let raw_estimate = zone_bytes + tile_bytes;
    Ok((raw_estimate as f64 * safety_factor()) as u64)
}

/// The size a reservation made via `estimate_analysis_bytes_precise` (or `estimate_analysis_bytes`)
/// can safely shrink to once `evaluate_points` (or `to_full`) has returned successfully: only the
/// zone/intersection arrays in the returned `PointEvaluationOutcomeFull` are still resident at
/// that point — the terrain grids and obstruction rasters used to compute them are transient
/// locals, already dropped before the function returns. Callers should call
/// `Reservation::shrink_to` with this value immediately after a successful call, before doing
/// further work (serialization, storage, streaming) that the reservation still needs to cover, now
/// at this much smaller size.
pub fn estimate_analysis_result_bytes(input: &PointEvaluationInput) -> u64 {
    let (rows_full, cols_full) = fresnel_zone_dims(input, ALPHA_ZONE_FULL);
    let (rows_inner, cols_inner) = fresnel_zone_dims(input, ALPHA_ZONE_INNER);
    let zone_cells = (rows_full as u64 * cols_full as u64) + (rows_inner as u64 * cols_inner as u64);
    ((zone_cells * BYTES_PER_ZONE_CELL_RESULT) as f64 * safety_factor()) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::coords::{GPSCoords3, NYSCoords3};
    use crate::types::obstructions::{ObstructionMeta, ObstructionRaster, ObstructionTypesFilter};
    use crate::types::tiles::TileId;
    use crate::util::coord_conversion::CoordinateConverter;
    use ndarray::Array2;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn gps_to_nys(lat: f64, lon: f64, alt_m: f64) -> NYSCoords3 {
        CoordinateConverter::new().to_nys_plane3(&GPSCoords3::new(lat, lon, alt_m))
    }

    fn make_input(pa: NYSCoords3, pb: NYSCoords3, freq: f64) -> PointEvaluationInput {
        PointEvaluationInput::new(pa, pb, freq, ObstructionTypesFilter::All)
    }

    /// Reports the same fixed number of synthetic obstructions (all `ActivePermits`) for every
    /// tile it's asked about.
    struct FlatCountObstructionProvider {
        count_per_tile: usize,
    }

    #[async_trait]
    impl ObstructionProvider for FlatCountObstructionProvider {
        async fn get_obstruction_ids_for_tile(
            &self,
            _tile_id: TileId,
        ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
            let mut out = HashMap::new();
            if self.count_per_tile > 0 {
                out.insert(
                    ObstructionType::ActivePermits,
                    (0..self.count_per_tile).map(|_| Uuid::new_v4()).collect(),
                );
            }
            Ok(out)
        }

        async fn get_obstruction_meta(
            &self,
            _obstruction_type: &ObstructionType,
            _obstruction_id: ObstructionId,
        ) -> Result<ObstructionMeta, AssetErr> {
            unreachable!("not exercised by these tests")
        }

        async fn get_obstruction_raster(
            &self,
            _obstruction_type: &ObstructionType,
            _obstruction_id: ObstructionId,
        ) -> Result<ObstructionRaster, AssetErr> {
            unreachable!("not exercised by these tests")
        }
    }

    struct FailingObstructionProvider;

    #[async_trait]
    impl ObstructionProvider for FailingObstructionProvider {
        async fn get_obstruction_ids_for_tile(
            &self,
            _tile_id: TileId,
        ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
            Err(AssetErr::AssetDownloadError("simulated failure".into()))
        }

        async fn get_obstruction_meta(
            &self,
            _obstruction_type: &ObstructionType,
            _obstruction_id: ObstructionId,
        ) -> Result<ObstructionMeta, AssetErr> {
            unreachable!("not exercised by these tests")
        }

        async fn get_obstruction_raster(
            &self,
            _obstruction_type: &ObstructionType,
            _obstruction_id: ObstructionId,
        ) -> Result<ObstructionRaster, AssetErr> {
            unreachable!("not exercised by these tests")
        }
    }

    fn short_link_input() -> PointEvaluationInput {
        make_input(
            gps_to_nys(40.700, -73.960, 30.0),
            gps_to_nys(40.705, -73.950, 30.0),
            5_000_000_000.0,
        )
    }

    #[tokio::test]
    async fn precise_estimate_scales_with_real_obstruction_count() {
        let input = short_link_input();
        let sparse = FlatCountObstructionProvider { count_per_tile: 1 };
        let dense = FlatCountObstructionProvider { count_per_tile: 15 };
        let sparse_estimate = estimate_analysis_bytes_precise(&input, &sparse).await.unwrap();
        let dense_estimate = estimate_analysis_bytes_precise(&input, &dense).await.unwrap();
        assert!(
            dense_estimate > sparse_estimate,
            "denser obstruction count should raise the estimate: sparse={sparse_estimate} dense={dense_estimate}"
        );
    }

    #[tokio::test]
    async fn precise_estimate_propagates_provider_errors() {
        let input = short_link_input();
        let provider = FailingObstructionProvider;
        let result = estimate_analysis_bytes_precise(&input, &provider).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn result_estimate_is_smaller_than_precise_peak_estimate() {
        let input = short_link_input();
        let provider = FlatCountObstructionProvider { count_per_tile: 5 };
        let peak = estimate_analysis_bytes_precise(&input, &provider).await.unwrap();
        let result = estimate_analysis_result_bytes(&input);
        assert!(
            result < peak,
            "post-hoc result estimate ({result}) should be smaller than the pre-admission peak estimate ({peak})"
        );
    }
}
