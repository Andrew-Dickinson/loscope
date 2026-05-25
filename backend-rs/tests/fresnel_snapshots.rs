#[path = "fresnel_snapshots/loader.rs"]
mod loader;

use loader::{assert_matches_snapshot, load_snapshot};
use loscope::analysis::fresnel_zone::compute_fresnel_zone;
use loscope::analysis::point_evaluation::{PointEvaluationInput};
use loscope::types::coords::NYSCoords3;
use loscope::types::obstructions::ObstructionTypesFilter;

macro_rules! snapshot_test {
    ($fn_name:ident, $bin:expr, $pa:expr, $pb:expr, $freq:expr, $alpha:expr) => {
        #[test]
        fn $fn_name() {
            let (pa, pb, freq, alpha) = ($pa, $pb, $freq, $alpha);
            let input = PointEvaluationInput::new(
                NYSCoords3::new(pa.0, pa.1, pa.2),
                NYSCoords3::new(pb.0, pb.1, pb.2),
                freq,
                ObstructionTypesFilter::All,
            );
            let zone = compute_fresnel_zone(&input, alpha);
            let snap = load_snapshot($bin);
            assert_matches_snapshot(&zone, &snap);
        }
    };
}

// NYS coordinates taken directly from snapshot_cases.py (already in usft, no GPS conversion)

snapshot_test!(ns_link_24ghz_alpha1_0,
    "ns_link_24ghz_alpha1_0",
    (1009748.3478422969, 253099.53772897943, 251.25),
    (1000565.7271487191, 241854.0, 257.6095239708276),
    24e9, 1.0);

snapshot_test!(ns_link_24ghz_alpha0_6,
    "ns_link_24ghz_alpha0_6",
    (1009748.3478422969, 253099.53772897943, 251.25),
    (1000565.7271487191, 241854.0, 257.6095239708276),
    24e9, 0.6);

snapshot_test!(diag_link_60ghz_alpha1_0,
    "diag_link_60ghz_alpha1_0",
    (1013771.106, 246714.567, 150.083),
    (1009758.4997793691, 253096.57297435577, 251.91700744628906),
    60e9, 1.0);

snapshot_test!(diag_link_60ghz_alpha0_6,
    "diag_link_60ghz_alpha0_6",
    (1013771.106, 246714.567, 150.083),
    (1009758.4997793691, 253096.57297435577, 251.91700744628906),
    60e9, 0.6);

snapshot_test!(ew_link_5ghz_alpha1_0,
    "ew_link_5ghz_alpha1_0",
    (1009948.5, 237111.5, 233.667),
    (998001.8977003782, 236324.25420103534, 291.61352244298905),
    5e9, 1.0);

snapshot_test!(ew_link_5ghz_alpha0_6,
    "ew_link_5ghz_alpha0_6",
    (1009948.5, 237111.5, 233.667),
    (998001.8977003782, 236324.25420103534, 291.61352244298905),
    5e9, 0.6);
