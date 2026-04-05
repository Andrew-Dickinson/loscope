"""Generate fresnel zone snapshot files for regression tests.

Runs compute_fresnel_zone for each test case and saves the full FresnelZone
arrays to tests/fresnel/snapshots/<name>_alpha<alpha>.npz.

Run once whenever the expected output intentionally changes:
    python tools/generate_fresnel_snapshots.py
"""
from __future__ import annotations

from pathlib import Path

import numpy as np

from los_analyzer.lib.fresnel.fresnel_zone2 import compute_fresnel_zone
from tests.fresnel.snapshot_cases import SNAPSHOT_CASES as CASES

SNAPSHOT_DIR = Path(__file__).parent.parent / "tests" / "fresnel" / "snapshots"


def main() -> None:
    SNAPSHOT_DIR.mkdir(parents=True, exist_ok=True)

    for name, pt_a, pt_b, freq in CASES:
        for alpha in (1.0, 0.6):
            zone = compute_fresnel_zone(pt_a, pt_b, freq, alpha=alpha)
            alpha_tag = f"alpha{alpha:.1f}".replace(".", "_")
            out_path = SNAPSHOT_DIR / f"{name}_{alpha_tag}.npz"
            np.savez_compressed(
                out_path,
                top=zone.top,
                bottom=zone.bottom,
                widths=zone.widths,
                offsets=zone.offsets,
                x_base_offset=np.array(zone.x_base_offset),
                y_base_offset=np.array(zone.y_base_offset),
            )
            print(f"wrote {out_path}  (top shape: {zone.top.shape})")


if __name__ == "__main__":
    main()
