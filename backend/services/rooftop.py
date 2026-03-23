"""Rooftop evaluation service.

Extracts the data pipeline from tools/evaluate_rooftop.py, returning a
JSON-serializable dict instead of writing files or HTML.
"""
from __future__ import annotations

import io
from pathlib import Path
from typing import Callable

import numpy as np


def build_terrain_obj_string(heightmap: np.ndarray) -> str:
    """Return OBJ file content for a building heightmap.

    Identical to tools/evaluate_rooftop.py::_build_obj_content().
    Coordinate system: X=local easting, Y=local northing, Z=elevation (ft).
    """
    z = heightmap.astype(np.float32) / 12.0
    W, H = z.shape
    z_floor = 0.0
    vi = 1
    buf = io.StringIO()
    buf.write("# Building heightmap terrain\n")
    buf.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
    buf.write("o heightmap\n\n")

    for xi in range(W):
        for yi in range(H):
            zt = float(z[xi, yi])
            if zt == 0.0:
                continue
            x0, y0 = float(xi), float(yi)
            x1, y1 = x0 + 1.0, y0 + 1.0

            buf.write(f"v {x0} {y0} {zt:.3f}\n")
            buf.write(f"v {x1} {y0} {zt:.3f}\n")
            buf.write(f"v {x1} {y1} {zt:.3f}\n")
            buf.write(f"v {x0} {y1} {zt:.3f}\n")
            o = vi
            buf.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            for dxi, dyi, ax, ay, bx, by in [
                ( 0, -1, x0, y0, x1, y0),
                ( 0, +1, x1, y1, x0, y1),
                (+1,  0, x1, y0, x1, y1),
                (-1,  0, x0, y1, x0, y0),
            ]:
                nxi, nyi = xi + dxi, yi + dyi
                if 0 <= nxi < W and 0 <= nyi < H:
                    nz = float(z[nxi, nyi])
                    zb = nz
                else:
                    nz = z_floor - 1.0
                    zb = z_floor

                if zt > nz and zb != 0.0:
                    buf.write(f"v {ax} {ay} {zb:.3f}\n")
                    buf.write(f"v {bx} {by} {zb:.3f}\n")
                    buf.write(f"v {bx} {by} {zt:.3f}\n")
                    buf.write(f"v {ax} {ay} {zt:.3f}\n")
                    o = vi
                    buf.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

    return buf.getvalue()


def run_rooftop_service(
    bin_id: str,
    gps_b: tuple[float, float, float],
    frequency_hz: float,
    progress_cb: Callable[[int, str], None],
    db_path: Path,
    tile_dir: Path,
    obstruction_dir: Path | None,
    sample_spacing: int = 5,
    mast_offset: float = 0.0,
    job_id: str | None = None,
    cache_dir: Path | None = None,
) -> dict:
    """Run the rooftop evaluation pipeline and return a JSON-serializable dict.

    Returns:
        {
            "bin_id": str,
            "job_id": str,   (filled by caller — placeholder here)
            "x_sw": int,
            "y_sw": int,
            "display_points": [{"x", "y", "z", "s", "nys_e", "nys_n", "nys_z"}, ...],
            "points": [{"x", "y", "z", "s", "nys_e", "nys_n", "nys_z"}, ...],
            "summary": {"n_clear", "n_partial", "n_full", "total"},
        }
        OBJ written to cache_dir/rooftop/<job_id>/terrain.obj (path stored in result).
    """
    from los_analyzer.building.heightmap import build_building_heightmap, filter_heightmap_outliers
    from los_analyzer.evaluation.rooftop import ObstructionStatus, evaluate_sample_points
    from los_analyzer.fresnel.fresnel_zone2 import translate_to_nys_plane
    from los_analyzer.sample_points import apply_mast_offset, generate_sample_points

    progress_cb(5, "Building heightmap…")
    heightmap, mask, poly_nys, x_sw, y_sw, _ = build_building_heightmap(
        bin_id, db_path, tile_dir
    )
    heightmap = filter_heightmap_outliers(heightmap, mask)

    progress_cb(25, "Generating sample points…")
    raw_pts = generate_sample_points(
        heightmap, x_sw, y_sw, sample_spacing,
        mask=mask, polygon=poly_nys,
    )
    display_pts, measurement_pts = apply_mast_offset(raw_pts, mast_offset)

    progress_cb(40, f"Evaluating {len(measurement_pts)} points…")
    [(common_e, common_n, common_z)] = translate_to_nys_plane([gps_b])
    common_pt_nys = (float(common_e), float(common_n), float(common_z))

    evaluations = evaluate_sample_points(
        measurement_pts,
        common_pt_nys,
        frequency_hz,
        tile_dir,
        obstruction_dir=obstruction_dir,
        obstruction_types="*",
    )

    progress_cb(80, "Building terrain mesh…")
    obj_content = build_terrain_obj_string(heightmap)

    # Summary
    n_clear   = sum(1 for e in evaluations if e.status == ObstructionStatus.UNOBSTRUCTED)
    n_partial = sum(1 for e in evaluations if e.status == ObstructionStatus.PARTIALLY_OBSTRUCTED)
    n_full    = sum(1 for e in evaluations if e.status == ObstructionStatus.FULLY_OBSTRUCTED)

    # Build point lists (local coords for Three.js + absolute NYS for tile-map requests)
    display_points = [
        {
            "x": round(float(dp[0]) - x_sw, 3),
            "y": round(float(dp[1]) - y_sw, 3),
            "z": round(float(dp[2]), 3),
            "s": ev.status.value,
            "nys_e": round(float(dp[0]), 3),
            "nys_n": round(float(dp[1]), 3),
            "nys_z": round(float(dp[2]), 3),
        }
        for ev, dp in zip(evaluations, display_pts)
    ]
    meas_points = [
        {
            "x": round(float(mp[0]) - x_sw, 3),
            "y": round(float(mp[1]) - y_sw, 3),
            "z": round(float(mp[2]), 3),
            "s": ev.status.value,
            "nys_e": round(float(mp[0]), 3),
            "nys_n": round(float(mp[1]), 3),
            "nys_z": round(float(mp[2]), 3),
        }
        for ev, mp in zip(evaluations, measurement_pts)
    ]

    result: dict = {
        "bin_id": bin_id,
        "job_id": "",   # filled by app.py after job_id is known
        "x_sw": int(x_sw),
        "y_sw": int(y_sw),
        "display_points": display_points,
        "points": meas_points,
        "summary": {
            "n_clear": n_clear,
            "n_partial": n_partial,
            "n_full": n_full,
            "total": len(evaluations),
        },
        # Far-end NYS coords — used by frontend to request tile-map
        "_nys_b": [float(common_e), float(common_n), float(common_z)],
    }

    # Write OBJ to disk (too large to embed in JSON response)
    if cache_dir is not None:
        import hashlib
        obj_hash = job_id or hashlib.sha256(obj_content[:4096].encode()).hexdigest()[:12]
        obj_dir = cache_dir / "rooftop" / obj_hash
        obj_dir.mkdir(parents=True, exist_ok=True)
        obj_path = obj_dir / "terrain.obj"
        obj_path.write_text(obj_content)
        result["_obj_hash"] = obj_hash

    progress_cb(95, "Done")
    return result
