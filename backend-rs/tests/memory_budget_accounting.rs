//! Verifies that `analysis::memory_estimate`'s estimator functions actually bound the *real*
//! peak heap allocation of the code paths they exist to budget for -- rather than trusting the
//! hand-derived formulas in memory_estimate.rs by inspection alone.
//!
//! Gated behind the `memory-audit` feature (see Cargo.toml's `[[test]]` entry) so it never runs
//! as part of a normal `cargo test`: cases here deliberately allocate up to hundreds of MB (a few
//! up to multiple GB) to stress the *real* code paths, and the counting global allocator this file
//! installs adds bookkeeping to every allocation for the whole process. Run explicitly with:
//!
//!   cargo test --features memory-audit --test memory_budget_accounting -- --test-threads=1 --nocapture
//!
//! Structure: each `#[test]`/`proptest!` case drives one *real* allocation-heavy function (not a
//! reimplementation of it) with synthetic-but-representative inputs via mock providers (see
//! `mocks.rs`), measures actual peak heap growth via `alloc_tracker::measure[_async]`, and asserts
//! it against the corresponding `memory_estimate.rs` function. A failing assertion here means a
//! real request shape exists whose actual memory use exceeds what the server would reserve for
//! it -- the exact condition that lets a request slip past the budget and contribute to an OOM.

#[path = "memory_budget_accounting/alloc_tracker.rs"]
mod alloc_tracker;
#[path = "memory_budget_accounting/mocks.rs"]
mod mocks;
#[path = "memory_budget_accounting/strategies.rs"]
mod strategies;

use alloc_tracker::{block_on, measure, measure_async};
use loscope::analysis::intersection_vis::tile_intersection_to_png;
use loscope::analysis::memory_estimate::{
    elevation_tile_endpoint_bytes, estimate_analysis_bytes_precise, estimate_full_recompute_bytes,
    estimate_heightmap_bytes, intersection_visualization_png_bytes, obstruction_obj_endpoint_bytes,
    ortho_tile_endpoint_bytes,
};
use loscope::analysis::point_evaluation::{
    PointEvaluationOutcomeLite, PointEvaluationOutput, ResultStatus, evaluate_points,
};
use loscope::building::bin_id::BINId;
use loscope::building::heightmap::RooftopHeightMapFactory;
use loscope::providers::ortho_provider::{CachingOrthoProvider, OrthoProvider};
use loscope::providers::terrain_classification_tile_provider::TerrainClassificationTile;
use loscope::types::coords::NYSCoords3;
use loscope::types::obstructions::{ObstructionMeta, ObstructionRaster, ObstructionType};
use loscope::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use loscope::util::image_adjustments::{apply_photo_adjustments, colorize_from_classifications};
use mocks::*;
use ndarray::Array2;
use proptest::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;
use uuid::Uuid;

fn tile_side() -> usize {
    usize::from(SUBGRID_TILE_SIDE_LENGTH_USFT)
}

/// Case count with regression-seed persistence disabled: this test binary lives under `tests/`,
/// not `src/`, so proptest can't find a `lib.rs`/`main.rs` to anchor a `proptest-regressions`
/// directory next to and prints a warning every run. We don't want it writing seed files into the
/// repo from an unexpected location anyway, so persistence is off explicitly rather than left to
/// that fallback behavior.
fn config(cases: u32) -> ProptestConfig {
    ProptestConfig { cases, failure_persistence: None, ..ProptestConfig::default() }
}

// ── estimate_analysis_bytes / evaluate_points ───────────────────────────────────────────────

proptest! {
    #![proptest_config(config(24))]

    /// Typical production traffic shape: modest links (up to ~3 miles), common ISP frequencies,
    /// no obstructions. Isolates the terrain/zone allocation math from obstruction accounting.
    /// Validates `estimate_analysis_bytes_precise`, the estimator `point_analysis` (the
    /// `/analyzePointPair` handler) actually calls.
    #[test]
    fn analysis_typical_links_no_obstructions_stay_within_estimate(
        input in strategies::point_pair_strategy(200.0, 15_000.0, 500e6, 6e9)
    ) {
        let elev = FlatElevationProvider { value: 300 };
        let obs = NoObstructions;
        let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
        let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
        prop_assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes (typical link, no obstructions)",
            sample.delta_bytes, estimate
        );
    }
}

proptest! {
    #![proptest_config(config(10))]

    /// Longer links at lower (but not pathologically low) frequencies -- on the expensive end of
    /// what a real customer request could plausibly specify. Deliberately does NOT explore all the
    /// way down to MIN_ANALYSIS_FREQUENCY (1kHz): a manual probe of estimate_analysis_bytes showed
    /// that combined with long distances reaches multi-hundred-GB *estimates* very quickly (e.g.
    /// ~265GB at 400,000usft @ 1MHz), which the existing `extreme_long_low_frequency_link_is_huge`
    /// unit test already covers analytically/cheaply. Actually allocating anywhere near that in
    /// this harness would thrash or OOM the machine running the suite (confirmed the hard way --
    /// an earlier, wider version of this range hung the test process), so this tier is capped at a
    /// worst corner (50,000usft @ 50MHz) that lands under 2GB.
    #[test]
    fn analysis_long_low_freq_links_stay_within_estimate(
        input in strategies::point_pair_strategy(20_000.0, 50_000.0, 50e6, 500e6)
    ) {
        let elev = FlatElevationProvider { value: 300 };
        let obs = NoObstructions;
        let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
        let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
        prop_assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes (long/low-freq link, no obstructions)",
            sample.delta_bytes, estimate
        );
    }
}

proptest! {
    #![proptest_config(config(24))]

    /// Dense-obstruction stress test: fixes a small, cheap link geometry (so tile count is small
    /// and predictable) and varies obstruction *density* and *raster size* per tile, up to and
    /// including the true theoretical extreme (500x500px is a full tile -- `ObstructionRaster`
    /// enforces no size cap, so this is legal, not contrived). This is the test that originally
    /// caught `estimate_analysis_bytes`'s flat per-tile padding being far too small for dense
    /// real-world obstruction data. `estimate_analysis_bytes_precise` (what `point_analysis` and
    /// the `get_full`-triggering endpoints actually call now) replaces that flat guess with the
    /// real per-tile obstruction count from the obstruction index, which is what lets this range
    /// run all the way to the theoretical max with real margin instead of needing to stay capped
    /// well below it — see `PER_OBSTRUCTION_BYTES_ESTIMATE`'s doc comment in memory_estimate.rs.
    #[test]
    fn analysis_dense_obstructions_stay_within_estimate(
        obstructions_per_tile in 1usize..=16,
        raster_w in 20usize..=500,
        raster_h in 20usize..=500,
    ) {
        let point_a = NYSCoords3::new(600_000.0, 300_000.0, 100.0);
        let point_b = NYSCoords3::new(600_800.0, 300_600.0, 100.0);
        let input = loscope::analysis::point_evaluation::PointEvaluationInput::new(
            point_a, point_b, 2_400_000_000.0,
            loscope::types::obstructions::ObstructionTypesFilter::All,
        );
        let elev = FlatElevationProvider { value: 300 };
        let obs = DenseObstructionProvider::new(obstructions_per_tile, raster_w, raster_h);
        let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
        let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
        prop_assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes ({obstructions_per_tile} obstructions/tile, {raster_w}x{raster_h} raster)",
            sample.delta_bytes, estimate
        );
    }
}

/// The proptest property above samples its (obstructions_per_tile, raster_w, raster_h) space
/// randomly, which -- especially at a modest case count -- isn't guaranteed to actually hit the
/// exact theoretical extreme. Pin it down explicitly: 16 obstructions in one tile, each a full
/// 500x500px raster (the largest `ObstructionRaster::read_from_tiff` will ever accept, since it
/// enforces no cap), covered with real (~2x) margin now that the estimate is count-aware.
#[test]
fn analysis_dense_obstructions_extreme_corner_stays_within_estimate() {
    let point_a = NYSCoords3::new(600_000.0, 300_000.0, 100.0);
    let point_b = NYSCoords3::new(600_800.0, 300_600.0, 100.0);
    let input = loscope::analysis::point_evaluation::PointEvaluationInput::new(
        point_a, point_b, 2_400_000_000.0,
        loscope::types::obstructions::ObstructionTypesFilter::All,
    );
    let elev = FlatElevationProvider { value: 300 };
    let obs = DenseObstructionProvider::new(16, 500, 500);
    let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
    let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
    assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
    assert!(
        sample.delta_bytes <= estimate,
        "measured {} bytes > estimate {} bytes (16 obstructions/tile, 500x500 rasters -- \
         theoretical extreme, ObstructionRaster has no smaller enforced cap)",
        sample.delta_bytes, estimate
    );
}

proptest! {
    #![proptest_config(config(8))]

    /// Confirms `estimate_analysis_bytes` (via `fresnel_zone_dims`, documented as "exact, not
    /// estimated") still bounds real allocation in the near-due-east/west geometry regime where
    /// `AngleContext`'s `1/sin_theta` term inflates `max_width` -- see
    /// `strategies::near_east_west_point_pair_strategy` for why this regime exists at all. This is
    /// the case most likely to reveal a genuine under-count if the "exact" claim doesn't actually
    /// hold at this extreme, since the real array size and the estimate are computed by two
    /// separate (if supposedly kept-in-sync) code paths.
    #[test]
    fn analysis_near_east_west_links_stay_within_estimate(
        input in strategies::near_east_west_point_pair_strategy(2_000.0, 2e9, 6e9)
    ) {
        let elev = FlatElevationProvider { value: 300 };
        let obs = NoObstructions;
        let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
        let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
        prop_assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes (near-east-west link)",
            sample.delta_bytes, estimate
        );
    }
}

/// Exact due-east link (two endpoints with *identical* northing, dy == 0.0 exactly) used to crash
/// `fresnel_zone.rs::integer_grid`'s `assert!(lo <= hi)`: dy == 0.0 drove `sin_theta` to exactly
/// 0, which sent `2.0 * semi_minor / sin_theta` to +-infinity/NaN (NaN comparisons are always
/// false, so the assert failed). Fixed by nudging the endpoint by 0.1 usft when delta.1 == 0.0,
/// mirroring the existing delta.0 == 0.0 guard (see `fresnel_zone.rs`, and
/// `test_fresnel_zone_identical_northing_does_not_panic` there for the direct regression test).
/// This test confirms the fix all the way through the actual memory-accounting path: now that the
/// due-east case runs instead of panicking, its real allocation still needs to stay within what
/// `estimate_analysis_bytes_precise` (what `point_analysis` actually calls) predicts for it.
#[test]
fn exact_due_east_link_stays_within_estimate() {
    let point_a = NYSCoords3::new(600_000.0, 300_000.0, 100.0);
    let point_b = NYSCoords3::new(615_000.0, 300_000.0, 100.0); // identical northing => dy == 0.0
    let input = loscope::analysis::point_evaluation::PointEvaluationInput::new(
        point_a, point_b, 2_400_000_000.0,
        loscope::types::obstructions::ObstructionTypesFilter::All,
    );
    let obs = NoObstructions;
    let elev = FlatElevationProvider { value: 300 };
    let estimate = block_on(estimate_analysis_bytes_precise(&input, &obs)).unwrap();
    let (result, sample) = measure_async(evaluate_points(Uuid::new_v4(), input, &elev, &obs));
    assert!(result.is_ok(), "evaluate_points failed unexpectedly: {:?}", result.err());
    assert!(
        sample.delta_bytes <= estimate,
        "measured {} bytes > estimate {} bytes (exact due-east link)",
        sample.delta_bytes, estimate
    );
}

// ── estimate_full_recompute_bytes / PointEvaluationOutcomeLite::to_full ─────────────────────

proptest! {
    #![proptest_config(config(16))]

    /// `estimate_full_recompute_bytes` mirrors `estimate_analysis_bytes_precise` for the
    /// `get_full` recompute path (see its doc comment in memory_estimate.rs) -- this confirms it
    /// actually bounds `to_full`'s real allocation the same way the direct `evaluate_points`
    /// tests above confirm for `analyzePointPair`. All four `get_full`-triggering endpoints now
    /// reserve via this before calling it — see `to_full_recompute_is_budgeted_by_all_four_endpoints`.
    #[test]
    fn full_recompute_estimate_covers_to_full(
        input in strategies::point_pair_strategy(200.0, 15_000.0, 500e6, 6e9)
    ) {
        let elev = FlatElevationProvider { value: 300 };
        let obs = NoObstructions;
        let estimate = block_on(estimate_full_recompute_bytes(&input, &obs)).unwrap();
        let output = PointEvaluationOutput::new(Uuid::new_v4(), input, ResultStatus::Unobstructed);
        let lite = PointEvaluationOutcomeLite::new(output, HashSet::new());
        let (result, sample) = measure_async(lite.to_full(&elev, &obs));
        prop_assert!(result.is_ok(), "to_full failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes (to_full recompute)",
            sample.delta_bytes, estimate
        );
    }
}

/// Regression-proofs the fix for the gap the manual audit this suite followed up on originally
/// found: all four endpoints that can trigger `to_full`'s recompute (`overview`,
/// `intersectionVisualization`, `fresnelSliceObj`, `fresnelKml` in endpoints/analysis.rs) must
/// call `try_reserve` before calling `get_full`. This test doesn't exercise HTTP routing
/// directly; it checks at the source level (without spinning up Rocket) that each handler body
/// still contains a `try_reserve` call, so a future refactor that accidentally drops it is
/// caught here rather than silently reopening the gap.
#[test]
fn to_full_recompute_is_budgeted_by_all_four_endpoints() {
    let analysis_endpoints_src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/endpoints/analysis.rs"),
    )
    .expect("failed to read src/endpoints/analysis.rs");

    for (fn_name, handler) in [
        ("map_overview", extract_fn_body(&analysis_endpoints_src, "map_overview")),
        ("intersection_visualization", extract_fn_body(&analysis_endpoints_src, "intersection_visualization")),
        ("get_fresnel_slice_obj", extract_fn_body(&analysis_endpoints_src, "get_fresnel_slice_obj")),
        ("fresnel_kml", extract_fn_body(&analysis_endpoints_src, "fresnel_kml")),
    ] {
        let handler = handler.unwrap_or_else(|| panic!("could not locate fn {fn_name} in endpoints/analysis.rs -- has it been renamed/removed?"));
        assert!(
            handler.contains("try_reserve"),
            "fn {fn_name} no longer calls try_reserve before its get_full call -- this reopens \
             the memory-budget gap this test exists to catch (see the audit history for context)"
        );
    }
}

fn extract_fn_body(src: &str, fn_name: &str) -> Option<String> {
    let needle = format!("fn {fn_name}(");
    let start = src.find(&needle)?;
    let body_start = src[start..].find('{')? + start;
    let mut depth = 0i32;
    for (i, c) in src[body_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[body_start..=body_start + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

// ── estimate_heightmap_bytes / RooftopHeightMapFactory::create ─────────────────────────────

proptest! {
    #![proptest_config(config(24))]

    /// Sweeps footprint bounding-box shapes across the full range `get_intersecting_tiles` allows
    /// (up to MAX_TILES_PER_BUILDING_FOOTPRINT=500 tiles), including extreme aspect ratios (e.g.
    /// one tile tall by 500 tiles wide), which is exactly the shape most likely to catch the
    /// estimator's per-pixel byte constant being wrong for unusual footprints.
    #[test]
    fn heightmap_bytes_covers_full_footprint_shape_range(
        (width_usft, height_usft) in strategies::footprint_bounds_strategy()
    ) {
        let base_easting = 600_000.0;
        let base_northing = 300_000.0;
        let footprint = strategies::rect_footprint(base_easting, base_northing, width_usft, height_usft);
        let bin_id = BINId::parse("1000001").unwrap();

        let bounds = loscope::building::heightmap::get_intersecting_tiles(&footprint)
            .expect("footprint_bounds_strategy should only produce accepted footprints")
            .1;
        let (output_w, output_h) = loscope::building::heightmap::heightmap_pixel_dims(&bounds);
        let estimate = estimate_heightmap_bytes(output_w, output_h);

        let fp = FixedFootprintProvider { polygon: footprint };
        let elev = FlatElevationProvider { value: 300 };
        let factory = RooftopHeightMapFactory::new(&fp, &elev);
        let (result, sample) = measure_async(factory.create(bin_id));
        prop_assert!(result.is_ok(), "RooftopHeightMapFactory::create failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes ({output_w}x{output_h} px heightmap)",
            sample.delta_bytes, estimate
        );
    }
}

// ── elevation_tile_endpoint_bytes ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(config(8))]

    /// elevation_tile_endpoint_bytes covers a fixed-size (SUBGRID_TILE_SIDE_LENGTH_USFT^2) tile
    /// fetch + TIFF re-encode, used by both get_terrain_raster and background_tile_raster. Pixel
    /// *content* shouldn't materially change the encoded size (uncompressed Gray16 TIFF), but a
    /// few varied values are cheap insurance against that assumption being wrong.
    #[test]
    fn elevation_tile_bytes_covers_fetch_and_reencode(value in any::<u16>()) {
        let estimate = elevation_tile_endpoint_bytes();
        let tile_id = TileId::parse("500300_00").unwrap();
        let side = tile_side();
        let (result, sample) = measure(|| {
            let tile = loscope::providers::elevation_tile_provider::ElevationTile::new(
                tile_id, Array2::from_elem((side, side), value),
            );
            let mut tiff_bytes = Vec::<u8>::with_capacity(2 * side * side);
            tile.write_to_tiff(std::io::Cursor::new(&mut tiff_bytes)).map(|_| tiff_bytes)
        });
        prop_assert!(result.is_ok(), "write_to_tiff failed unexpectedly: {:?}", result.err());
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes (elevation tile fetch+reencode)",
            sample.delta_bytes, estimate
        );
    }
}

// ── obstruction_obj_endpoint_bytes ──────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(config(20))]

    /// get_terrain_obstruction_obj's real cost is one ObstructionRaster (fetched) + one clone
    /// (`to_obj_stream` clones the heightmap internally) + small bounded string-chunk buffers.
    /// Sweeps raster dimensions well past any real building footprint (existing fixtures are
    /// ~150-230px) to find the actual breakeven point against the flat 16MB (pre-safety-factor)
    /// allowance.
    #[test]
    fn obstruction_obj_bytes_covers_fetch_and_stream_clone(
        raster_w in 50usize..=3000,
        raster_h in 50usize..=3000,
    ) {
        let estimate = obstruction_obj_endpoint_bytes();
        let obstruction_id = Uuid::new_v4();
        let obstruction_type = ObstructionType::ActivePermits;
        let raster = ObstructionRaster::new(Array2::<u16>::zeros((raster_w, raster_h)));
        let meta = ObstructionMeta::new(
            obstruction_id, obstruction_type.clone(), std::collections::HashMap::new(),
            loscope::types::coords::NYSCoords2::new(0.0, 0.0), vec![],
            raster_w as u64, raster_h as u64, None,
        );
        let _ = &meta;

        let (result, sample) = measure_async(async {
            use futures_util::StreamExt;
            let mut stream = std::pin::pin!(raster.to_obj_stream(obstruction_type, obstruction_id, 0, 0));
            let mut total_len = 0usize;
            while let Some(chunk) = stream.next().await {
                total_len += chunk.len();
            }
            total_len
        });
        let _ = result;
        prop_assert!(
            sample.delta_bytes <= estimate,
            "measured {} bytes > estimate {} bytes ({raster_w}x{raster_h} obstruction raster)",
            sample.delta_bytes, estimate
        );
    }
}

// ── intersection_visualization_png_bytes ────────────────────────────────────────────────────

#[test]
fn intersection_visualization_png_bytes_covers_measured_allocation() {
    let estimate = intersection_visualization_png_bytes();
    let side = tile_side();
    // A fully-obstructed tile (every cell non-zero) is the worst case for tile_intersection_to_png
    // (the early-return for an all-zero tile allocates nothing).
    let grid: Array2<Option<&loscope::types::fraction::FractionU8>> =
        Array2::from_elem((side, side), None);
    let full = loscope::types::fraction::FractionU8::new(1.0f64);
    let grid = grid.mapv(|_| Some(&full));

    let (result, sample) = measure(|| tile_intersection_to_png(grid));
    assert!(result.is_some(), "expected a PNG for a fully-obstructed tile");
    assert!(
        sample.delta_bytes <= estimate,
        "measured {} bytes > estimate {} bytes (intersection visualization PNG)",
        sample.delta_bytes, estimate
    );
}

// ── ortho_tile_endpoint_bytes ────────────────────────────────────────────────────────────────

fn tile_002205() -> TileId {
    TileId::parse("002205_00").unwrap()
}

/// End-to-end, using the real fixture (tests/resources/002205.jp2, ~9.6MB compressed JP2 covering
/// a full 5000x5000px LAS-tile-sized source image) through the exact sequence
/// `get_terrain_ortho` runs: decode -> photo adjustments -> classification colorize -> (JPEG
/// encode omitted here since it's small and already covered by the general safety factor).
#[test]
fn ortho_bytes_covers_real_fixture_decode_and_adjust() {
    let estimate = ortho_tile_endpoint_bytes();
    let jp2_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/resources/002205.jp2");
    let asset_provider = FixedFileAssetProvider { path: jp2_path };
    let provider = CachingOrthoProvider::new(std::sync::Arc::new(asset_provider));

    let (result, sample) = measure_async(async {
        let img = provider.get_ortho(tile_002205()).await?;
        let img = apply_photo_adjustments(img);
        let classification = TerrainClassificationTile::new_empty(tile_002205());
        let img = colorize_from_classifications(img, classification);
        Ok::<_, loscope::types::errors::AssetErr>(img)
    });
    assert!(result.is_ok(), "ortho decode/adjust pipeline failed unexpectedly: {:?}", result.err());
    assert!(
        sample.delta_bytes <= estimate,
        "measured {} bytes > estimate {} bytes (real ortho fixture decode+adjust) -- see \
         ortho_raw_buffer_scales_with_source_file_size below for the isolated root cause if this fails",
        sample.delta_bytes, estimate
    );
}

proptest! {
    #![proptest_config(config(10))]

    /// Isolates CachingOrthoProvider::get_ortho's raw-file-buffer allocation
    /// (`Vec::with_capacity(asset_size)` + `read_to_end`, ortho_provider.rs) from JP2 decoding, by
    /// serving garbage (invalid-JP2) content of a controlled size: decoding will fail, but the
    /// buffer is allocated and fully populated *before* decoding is attempted, so the allocation
    /// happens regardless. This directly measures whether the raw source-file size alone --
    /// independent of the "4 decoded copies" ortho_tile_endpoint_bytes() actually budgets for --
    /// can exceed the endpoint's reservation.
    #[test]
    fn ortho_raw_buffer_scales_with_source_file_size(size_mb in 1u64..=20) {
        let size_bytes = (size_mb * 1024 * 1024) as usize;
        let path = write_garbage_file(size_bytes);
        let asset_provider = FixedFileAssetProvider { path: path.to_path_buf() };
        let provider = CachingOrthoProvider::new(std::sync::Arc::new(asset_provider));

        let (result, sample) = measure_async(provider.get_ortho(tile_002205()));
        // Decoding garbage is expected to fail -- we only care about the allocation that happened
        // on the way there.
        prop_assert!(result.is_err(), "expected garbage content to fail JP2 decoding");
        prop_assert!(
            sample.delta_bytes >= size_bytes as u64,
            "expected the raw {size_bytes}-byte file to be fully buffered in memory before decode \
             was attempted, but only measured {} bytes -- did the read path change to stream \
             instead of buffering the whole file?",
            sample.delta_bytes
        );
    }
}

/// Finds the actual breakeven point where the raw-file-buffer allocation
/// `ortho_raw_buffer_scales_with_source_file_size` isolated above starts exceeding
/// `ortho_tile_endpoint_bytes()`'s total reservation on its own, i.e. where the "4 decoded
/// copies" formula's safety-factor slack alone is no longer enough to absorb an unaccounted raw
/// source file. Prints the number (informative on its own -- compare against real production
/// ortho JP2 file sizes to judge headroom) and asserts it stays comfortably (2x) above the real
/// fixture this suite uses (tests/resources/002205.jp2, ~9.6MB) as a regression guard: if the
/// margin ever shrinks below that, either the estimate formula regressed or ortho source files
/// have grown, and either is worth knowing about before it becomes a live incident.
#[test]
fn ortho_raw_buffer_breakeven_point_against_endpoint_budget() {
    const REAL_FIXTURE_APPROX_MB: u64 = 10; // tests/resources/002205.jp2 is ~9.6MB
    const REQUIRED_MARGIN_MULTIPLIER: u64 = 2;

    let estimate = ortho_tile_endpoint_bytes();
    let mut lo_mb = 1u64;
    let mut hi_mb = 200u64;
    while lo_mb < hi_mb {
        let mid_mb = lo_mb + (hi_mb - lo_mb) / 2;
        let size_bytes = (mid_mb * 1024 * 1024) as usize;
        let path = write_garbage_file(size_bytes);
        let asset_provider = FixedFileAssetProvider { path: path.to_path_buf() };
        let provider = CachingOrthoProvider::new(std::sync::Arc::new(asset_provider));
        let (_, sample) = measure_async(provider.get_ortho(tile_002205()));
        if sample.delta_bytes > estimate {
            hi_mb = mid_mb;
        } else {
            lo_mb = mid_mb + 1;
        }
    }
    println!(
        "ortho_tile_endpoint_bytes() = {estimate} bytes ({:.1}MB); raw source file breakeven \
         point ~= {lo_mb}MB (real fixture is ~{REAL_FIXTURE_APPROX_MB}MB)",
        estimate as f64 / 1024.0 / 1024.0
    );
    assert!(
        lo_mb >= REAL_FIXTURE_APPROX_MB * REQUIRED_MARGIN_MULTIPLIER,
        "raw ortho source file breakeven point ({lo_mb}MB) no longer has a comfortable margin \
         over the real fixture size (~{REAL_FIXTURE_APPROX_MB}MB) -- the unaccounted raw-buffer \
         gap documented on ortho_tile_endpoint_bytes() is close to becoming a live risk"
    );
}

// ── sanity: strategies stay inside the coordinate system's own valid bounds ────────────────

proptest! {
    #![proptest_config(config(50))]

    #[test]
    fn point_pair_strategy_generates_valid_coordinates(
        input in strategies::point_pair_strategy(200.0, 15_000.0, 500e6, 6e9)
    ) {
        prop_assert!(input.point_a().valid(), "point_a out of valid NYS bounds: {:?}", input.point_a());
        prop_assert!(input.point_b().valid(), "point_b out of valid NYS bounds: {:?}", input.point_b());
    }

    #[test]
    fn footprint_bounds_strategy_stays_within_nys_bounds(
        (width_usft, height_usft) in strategies::footprint_bounds_strategy()
    ) {
        let footprint = strategies::rect_footprint(600_000.0, 300_000.0, width_usft, height_usft);
        prop_assert!(
            loscope::building::heightmap::get_intersecting_tiles(&footprint).is_ok(),
            "footprint_bounds_strategy produced a footprint get_intersecting_tiles rejects: {width_usft}x{height_usft}"
        );
    }
}
