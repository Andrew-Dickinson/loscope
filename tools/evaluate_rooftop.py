"""Evaluate line-of-sight status for rooftop sample points against a far-end input point.

Combines the building heightmap extraction and sample-point generation pipeline
from generate_building_sample_points.py with Fresnel zone obstruction evaluation
for each measurement position.

Usage:
    python tools/evaluate_rooftop.py BIN --input-point LAT LON ALT_M [options]

Environment variables (optional — needed only when tiles are not already cached):
    LOS_S3_BUCKET   S3 bucket containing preprocessed tiles
    LOS_S3_PREFIX   Key prefix, e.g. "nyc-lidar-2021/preprocessed"
"""
from __future__ import annotations

import argparse
import dataclasses
from pathlib import Path

import numpy as np
import tifffile

from los_analyzer.building.heightmap import (
    build_building_heightmap,
    fetch_building_geometry,
    filter_heightmap_outliers,
)
from los_analyzer.evaluation.rooftop import (
    ObstructionStatus,
    SamplePointEvaluation,
    evaluate_sample_points,
)
from los_analyzer.fresnel.fresnel_zone2 import translate_to_nys_plane
from los_analyzer.sample_points import apply_mast_offset, generate_sample_points


_CUBE_HALF = 0.125  # 3 inches = 0.25 ft; half-edge = 0.125 ft


@dataclasses.dataclass
class EvaluationResult:
    heightmap_path: Path
    mask_path: Path
    sample_pts_path: Path | None = None
    sample_pts_measurement_path: Path | None = None
    evaluation_path: Path | None = None
    terrain_obj_path: Path | None = None
    viewer_path: Path | None = None
    # Keyed by ObstructionStatus value string, e.g. "unobstructed"
    sample_pts_display_obj_paths: dict[str, Path] = dataclasses.field(default_factory=dict)
    sample_pts_measurement_obj_paths: dict[str, Path] = dataclasses.field(default_factory=dict)


def _build_fetcher(tile_dir: Path):
    """Return a CachingTileFetcher backed by S3, or None if not configured."""
    import os
    bucket = os.environ.get("LOS_S3_BUCKET")
    prefix = os.environ.get("LOS_S3_PREFIX")
    if not bucket or not prefix:
        return None
    from los_analyzer.tiles.fetch import CachingTileFetcher
    from los_analyzer.tiles.s3_backend import S3TileBackend
    return CachingTileFetcher(S3TileBackend(bucket, prefix), tile_dir)


def _build_obj_content(heightmap: np.ndarray) -> str:
    """Return the OBJ file content for a building heightmap as a string."""
    import io
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


def _export_heightmap_obj(heightmap: np.ndarray, out_path: Path) -> None:
    """Write a Minecraft-style voxel terrain OBJ from a building heightmap."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(_build_obj_content(heightmap))


def _export_sample_points_obj(
    pts: np.ndarray,
    x_sw: int,
    y_sw: int,
    out_path: Path,
) -> None:
    """Write sample points as 3-inch cubes to an OBJ file."""
    h = _CUBE_HALF
    out_path.parent.mkdir(parents=True, exist_ok=True)
    vi = 1

    with open(out_path, "w") as f:
        f.write(f"# Sample points — {len(pts)} points, each a 3-inch cube\n")
        f.write("# 1 unit = 1 US survey foot\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
        f.write("# Origin = SW corner of the heightmap — aligns with the terrain OBJ\n\n")
        f.write("o sample_points\n\n")

        for x_abs, y_abs, z in pts:
            cx = float(x_abs) - x_sw
            cy = float(y_abs) - y_sw
            cz = float(z)

            faces = (
                ((cx-h, cy-h, cz+h), (cx+h, cy-h, cz+h), (cx+h, cy+h, cz+h), (cx-h, cy+h, cz+h)),
                ((cx-h, cy+h, cz-h), (cx+h, cy+h, cz-h), (cx+h, cy-h, cz-h), (cx-h, cy-h, cz-h)),
                ((cx-h, cy-h, cz-h), (cx+h, cy-h, cz-h), (cx+h, cy-h, cz+h), (cx-h, cy-h, cz+h)),
                ((cx+h, cy+h, cz-h), (cx-h, cy+h, cz-h), (cx-h, cy+h, cz+h), (cx+h, cy+h, cz+h)),
                ((cx+h, cy-h, cz-h), (cx+h, cy+h, cz-h), (cx+h, cy+h, cz+h), (cx+h, cy-h, cz+h)),
                ((cx-h, cy+h, cz-h), (cx-h, cy-h, cz-h), (cx-h, cy-h, cz+h), (cx-h, cy+h, cz+h)),
            )
            for verts in faces:
                for vx, vy, vz in verts:
                    f.write(f"v {vx:.4f} {vy:.4f} {vz:.4f}\n")
                o = vi
                f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                vi += 4


def run_evaluation(
    bin_id: str,
    db_path: Path,
    tile_dir: Path,
    out_dir: Path,
    input_point_gps: tuple[float, float, float],  # (lat, lon, alt_m)
    frequency_hz: float = 24_000_000_000.0,
    sample_spacing: int = 5,
    mast_offset: float = 0.0,
    obstruction_dir: Path | None = None,
    export_obj: bool = False,
) -> EvaluationResult:
    """Run the full rooftop evaluation pipeline for a single building BIN.

    Args:
        bin_id: Building Identification Number.
        db_path: SQLite database containing building_footprints.
        tile_dir: Preprocessed LiDAR tile directory.
        out_dir: Output directory.
        input_point_gps: Far-end GPS coordinate ``(lat, lon, alt_m)``.
        frequency_hz: Link frequency in Hz.
        sample_spacing: XY sample grid spacing in feet (>= 1).
        mast_offset: Vertical offset in feet for the top sample point at each
            (X, Y) location.
        obstruction_dir: Directory of obstruction tif+json pairs (optional).
        export_obj: If True, write Minecraft-style OBJ files.

    Returns:
        :class:`EvaluationResult` with paths to all written files.
    """
    # 1. Optionally prefetch tiles from S3
    from los_analyzer.obstructions.building_footprints import _intersecting_tile_ids
    poly_for_fetch = fetch_building_geometry(bin_id, db_path)
    tile_ids_for_fetch = _intersecting_tile_ids(poly_for_fetch)

    fetcher = _build_fetcher(tile_dir)
    if fetcher is not None:
        missing = [t for t in tile_ids_for_fetch if not fetcher.is_cached(t)]
        if missing:
            print(f"Fetching {len(missing)} tile(s) from S3: {missing}")
            fetcher.ensure_tiles(missing)
    else:
        print("S3 not configured — using local tiles only.")

    # 2. Build heightmap + mask
    heightmap, mask, poly_nys, x_sw, y_sw, _ = build_building_heightmap(
        bin_id, db_path, tile_dir
    )
    heightmap = filter_heightmap_outliers(heightmap, mask)

    # 3. Write heightmap and mask TIFFs
    out_dir.mkdir(parents=True, exist_ok=True)
    heightmap_path = out_dir / f"{bin_id}_heightmap.tif"
    mask_path = out_dir / f"{bin_id}_mask.tif"
    tifffile.imwrite(str(heightmap_path), heightmap)
    tifffile.imwrite(str(mask_path), mask)

    # 4. Generate sample points
    raw_pts = generate_sample_points(
        heightmap, x_sw, y_sw, sample_spacing,
        mask=mask, polygon=poly_nys,
    )
    display_pts, measurement_pts = apply_mast_offset(raw_pts, mast_offset)

    sample_pts_path = out_dir / f"{bin_id}_sample_points.npy"
    sample_pts_measurement_path = out_dir / f"{bin_id}_sample_points_measurement.npy"
    np.save(str(sample_pts_path), display_pts)
    np.save(str(sample_pts_measurement_path), measurement_pts)

    # 5. Convert far-end GPS to NYS plane
    [(common_e, common_n, common_z)] = translate_to_nys_plane([input_point_gps])
    common_pt_nys = (float(common_e), float(common_n), float(common_z))

    # 6. Evaluate each measurement point
    evaluations = evaluate_sample_points(
        measurement_pts,
        common_pt_nys,
        frequency_hz,
        tile_dir,
        obstruction_dir=obstruction_dir,
        obstruction_types="*",
    )

    # 7. Save evaluations as structured array
    evaluation_path = out_dir / f"{bin_id}_evaluation.npy"
    _save_evaluations(evaluations, evaluation_path)

    # 8. Print summary
    n_clear = sum(1 for e in evaluations if e.status == ObstructionStatus.UNOBSTRUCTED)
    n_partial = sum(1 for e in evaluations if e.status == ObstructionStatus.PARTIALLY_OBSTRUCTED)
    n_full = sum(1 for e in evaluations if e.status == ObstructionStatus.FULLY_OBSTRUCTED)
    total = len(evaluations)
    print(f"Results ({total} points):")
    print(f"  Unobstructed:         {n_clear:4d}  ({100*n_clear/total:.1f}%)")
    print(f"  Partially obstructed: {n_partial:4d}  ({100*n_partial/total:.1f}%)")
    print(f"  Fully obstructed:     {n_full:4d}  ({100*n_full/total:.1f}%)")

    # 9. HTML viewer (always generated)
    viewer_path = out_dir / f"{bin_id}_viewer.html"
    _generate_html_viewer(heightmap, evaluations, display_pts, x_sw, y_sw, bin_id, viewer_path)

    # 10. Optional OBJ export — one file per obstruction status
    terrain_obj_path: Path | None = None
    display_obj_paths: dict[str, Path] = {}
    measurement_obj_paths: dict[str, Path] = {}
    if export_obj:
        terrain_obj_path = out_dir / f"{bin_id}_heightmap.obj"
        _export_heightmap_obj(heightmap, terrain_obj_path)

        # Group display and measurement points by evaluation status
        grouped_display: dict[str, list[np.ndarray]] = {}
        grouped_measurement: dict[str, list[np.ndarray]] = {}
        for ev, dp, mp in zip(evaluations, display_pts, measurement_pts):
            key = ev.status.value
            grouped_display.setdefault(key, []).append(dp)
            grouped_measurement.setdefault(key, []).append(mp)

        for key, pts_list in grouped_display.items():
            path = out_dir / f"{bin_id}_sample_points_display_{key}.obj"
            _export_sample_points_obj(np.array(pts_list), x_sw, y_sw, path)
            display_obj_paths[key] = path

        for key, pts_list in grouped_measurement.items():
            path = out_dir / f"{bin_id}_sample_points_measurement_{key}.obj"
            _export_sample_points_obj(np.array(pts_list), x_sw, y_sw, path)
            measurement_obj_paths[key] = path

    return EvaluationResult(
        heightmap_path=heightmap_path,
        mask_path=mask_path,
        sample_pts_path=sample_pts_path,
        sample_pts_measurement_path=sample_pts_measurement_path,
        evaluation_path=evaluation_path,
        terrain_obj_path=terrain_obj_path,
        viewer_path=viewer_path,
        sample_pts_display_obj_paths=display_obj_paths,
        sample_pts_measurement_obj_paths=measurement_obj_paths,
    )


_HTML_TEMPLATE = """\
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>LOS Viewer \u2014 BIN __BIN_ID__</title>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { background: #111827; overflow: hidden; font-family: monospace; }
    canvas { display: block; }
    #hud {
      position: absolute; top: 12px; left: 12px;
      color: #9ca3af; font-size: 12px; pointer-events: none;
    }
    #legend {
      position: absolute; top: 12px; right: 12px;
      background: rgba(0,0,0,0.65); color: #e5e7eb;
      padding: 10px 14px; border-radius: 6px;
      font-size: 13px; line-height: 2;
      border: 1px solid rgba(255,255,255,0.1);
    }
    .dot { display: inline-block; width: 11px; height: 11px;
           border-radius: 50%; margin-right: 6px; vertical-align: middle; }
  </style>
</head>
<body>
  <div id="hud">BIN __BIN_ID__ &nbsp;&middot;&nbsp; Drag to orbit &nbsp;&middot;&nbsp; Scroll to zoom</div>
  <div id="legend">
    <b>LOS Status</b><br>
    <span class="dot" style="background:#22cc44"></span>Unobstructed (__N_CLEAR__)<br>
    <span class="dot" style="background:#ffcc00"></span>Partial (__N_PARTIAL__)<br>
    <span class="dot" style="background:#ff4444"></span>Fully blocked (__N_FULL__)
  </div>
  <script type="importmap">
  {"imports":{"three":"https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.js","three/addons/":"https://cdn.jsdelivr.net/npm/three@0.160.0/examples/jsm/"}}
  </script>
  <script type="module">
    import * as THREE from 'three';
    import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

    // ── Renderer ────────────────────────────────────────────────────────────
    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(devicePixelRatio);
    renderer.setSize(innerWidth, innerHeight);
    document.body.appendChild(renderer.domElement);

    // ── Scene / camera ──────────────────────────────────────────────────────
    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x111827);

    const camera = new THREE.PerspectiveCamera(55, innerWidth / innerHeight, 0.5, 50000);
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;

    // ── Lighting ────────────────────────────────────────────────────────────
    scene.add(new THREE.AmbientLight(0xffffff, 0.6));
    const sun = new THREE.DirectionalLight(0xffeedd, 1.0);
    sun.position.set(1, 3, 2);
    scene.add(sun);

    // ── Sample point data (needed for vertex colours below) ──────────────────
    const STATUS_HEX = {
      unobstructed:        0x22cc44,
      partially_obstructed:0xffcc00,
      fully_obstructed:    0xff4444,
    };
    const POINTS = __POINTS_JSON__;

    // ── OBJ parser ──────────────────────────────────────────────────────────
    // OBJ vertex layout: v x_local  y_local  z_feet
    // Three.js Y-up mapping:  threeX = x_local  threeY = z_feet  threeZ = -y_local
    function parseOBJ(text) {
      const pos = [], idx = [];
      for (const raw of text.split('\\n')) {
        const p = raw.trimStart().split(' ');
        if (p[0] === 'v') {
          pos.push(parseFloat(p[1]), parseFloat(p[3]), -parseFloat(p[2]));
        } else if (p[0] === 'f') {
          const v = p.slice(1).map(s => parseInt(s) - 1);
          idx.push(v[0], v[1], v[2]);
          if (v.length === 4) idx.push(v[0], v[2], v[3]);
        }
      }
      return { pos, idx };
    }

    const OBJ_TEXT = __OBJ_LITERAL__;
    const { pos, idx } = parseOBJ(OBJ_TEXT);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    geo.setIndex(idx);
    geo.computeVertexNormals();

    // ── Voronoi shader: per-display-pixel nearest sample-point status ────────
    // The fragment shader receives each rendered screen pixel's 3-D world
    // position and finds the nearest sample point in (easting, elevation,
    // −northing) space, producing a true Voronoi diagram at display resolution.
    const N_PTS = __N_PTS__;
    let terrainMat;
    if (N_PTS === 0) {
      terrainMat = new THREE.MeshLambertMaterial({ color: 0x94a3b8, side: THREE.DoubleSide });
    } else {
      const uPos = POINTS.map(pt => new THREE.Vector3(pt.x, pt.z, -pt.y));
      const uCol = POINTS.map(pt => {
        const h = STATUS_HEX[pt.s] ?? 0x94a3b8;
        return new THREE.Vector3((h >> 16 & 0xff) / 255, (h >> 8 & 0xff) / 255, (h & 0xff) / 255);
      });
      terrainMat = new THREE.ShaderMaterial({
        uniforms: { uPos: { value: uPos }, uCol: { value: uCol } },
        vertexShader: `
          varying vec3 vWorld;
          varying vec3 vNorm;
          void main() {
            vWorld = (modelMatrix * vec4(position, 1.0)).xyz;
            vNorm  = normalize(normalMatrix * normal);
            gl_Position = projectionMatrix * viewMatrix * vec4(position, 1.0);
          }`,
        fragmentShader: `
          #define N __N_PTS__
          uniform vec3 uPos[N];
          uniform vec3 uCol[N];
          varying vec3 vWorld;
          varying vec3 vNorm;
          void main() {
            float bestD2 = 1e15;
            vec3 col = vec3(0.58, 0.64, 0.72);
            for (int i = 0; i < N; i++) {
              vec3 d = vWorld - uPos[i];
              float d2 = dot(d, d);
              if (d2 < bestD2) { bestD2 = d2; col = uCol[i]; }
            }
            float diff = max(dot(normalize(vNorm), normalize(vec3(1.0, 3.0, 2.0))), 0.0);
            gl_FragColor = vec4(col * (0.5 + 0.5 * diff), 1.0);
          }`,
        side: THREE.DoubleSide,
      });
    }
    scene.add(new THREE.Mesh(geo, terrainMat));

    // ── Camera: fit to bounding box ─────────────────────────────────────────
    geo.computeBoundingBox();
    const box = geo.boundingBox;
    const center = new THREE.Vector3();
    box.getCenter(center);
    const size = new THREE.Vector3();
    box.getSize(size);
    const span = Math.max(size.x, size.z);
    camera.position.set(center.x - span * 0.6, center.y + span * 0.9, center.z + span * 1.3);
    camera.lookAt(center);
    controls.target.copy(center);
    controls.update();

    // ── Sample point spheres (instanced, grouped by status) ───────────────────
    const ptGeo = new THREE.SphereGeometry(0.8, 8, 6);
    const byStatus = {};
    for (const pt of POINTS) { (byStatus[pt.s] ??= []).push(pt); }
    const dummy = new THREE.Object3D();
    for (const [status, pts] of Object.entries(byStatus)) {
      const mesh = new THREE.InstancedMesh(ptGeo,
        new THREE.MeshBasicMaterial({ color: STATUS_HEX[status] ?? 0xffffff }),
        pts.length);
      pts.forEach((pt, i) => {
        // pt.x = east_local, pt.y = north_local, pt.z = elevation_feet
        dummy.position.set(pt.x, pt.z, -pt.y);
        dummy.updateMatrix();
        mesh.setMatrixAt(i, dummy.matrix);
      });
      mesh.instanceMatrix.needsUpdate = true;
      scene.add(mesh);
    }

    // ── Resize + render loop ─────────────────────────────────────────────────
    window.addEventListener('resize', () => {
      camera.aspect = innerWidth / innerHeight;
      camera.updateProjectionMatrix();
      renderer.setSize(innerWidth, innerHeight);
    });
    (function animate() { requestAnimationFrame(animate); controls.update(); renderer.render(scene, camera); })();
  </script>
</body>
</html>
"""


def _generate_html_viewer(
    heightmap: np.ndarray,
    evaluations: list[SamplePointEvaluation],
    display_pts: np.ndarray,
    x_sw: int,
    y_sw: int,
    bin_id: str,
    out_path: Path,
) -> None:
    """Write a self-contained Three.js HTML viewer for the heightmap and evaluation results."""
    import json

    obj_content = _build_obj_content(heightmap)
    # Embed as a JS string literal.  OBJ files contain no quotes or backslashes,
    # but escape just in case and wrap in double-quotes.
    obj_literal = json.dumps(obj_content)  # properly escaped JSON string

    # Build compact point list: {x, y, z} in local coords + status key "s"
    points = [
        {
            "x": round(float(dp[0]) - x_sw, 3),
            "y": round(float(dp[1]) - y_sw, 3),
            "z": round(float(dp[2]), 3),
            "s": ev.status.value,
        }
        for ev, dp in zip(evaluations, display_pts)
    ]
    points_json = json.dumps(points, separators=(",", ":"))

    n_clear   = sum(1 for e in evaluations if e.status == ObstructionStatus.UNOBSTRUCTED)
    n_partial = sum(1 for e in evaluations if e.status == ObstructionStatus.PARTIALLY_OBSTRUCTED)
    n_full    = sum(1 for e in evaluations if e.status == ObstructionStatus.FULLY_OBSTRUCTED)
    n_pts     = len(evaluations)

    html = (
        _HTML_TEMPLATE
        .replace("__BIN_ID__",      bin_id)
        .replace("__N_CLEAR__",     str(n_clear))
        .replace("__N_PARTIAL__",   str(n_partial))
        .replace("__N_FULL__",      str(n_full))
        .replace("__OBJ_LITERAL__", obj_literal)
        .replace("__POINTS_JSON__", points_json)
        .replace("__N_PTS__",       str(n_pts))
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(html, encoding="utf-8")


def _save_evaluations(evaluations: list[SamplePointEvaluation], out_path: Path) -> None:
    """Save evaluations as a structured numpy array."""
    dtype = np.dtype([
        ("easting", np.float64),
        ("northing", np.float64),
        ("z_feet", np.float64),
        ("status", "U32"),
        ("max_obstruction_alpha1", np.float64),
        ("max_obstruction_alpha06", np.float64),
    ])
    arr = np.empty(len(evaluations), dtype=dtype)
    for i, ev in enumerate(evaluations):
        arr[i]["easting"] = ev.point[0]
        arr[i]["northing"] = ev.point[1]
        arr[i]["z_feet"] = ev.point[2]
        arr[i]["status"] = ev.status.value
        arr[i]["max_obstruction_alpha1"] = ev.max_obstruction_alpha1
        arr[i]["max_obstruction_alpha06"] = ev.max_obstruction_alpha06
    np.save(str(out_path), arr)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Evaluate rooftop sample-point LOS status against a far-end input point"
    )
    parser.add_argument("bin", metavar="BIN", help="Building Identification Number")
    parser.add_argument(
        "--input-point",
        nargs=3,
        type=float,
        metavar=("LAT", "LON", "ALT_M"),
        required=True,
        help="Far-end GPS coordinate: latitude longitude altitude_meters",
    )
    parser.add_argument(
        "--frequency-hz",
        type=float,
        default=24_000_000_000.0,
        metavar="HZ",
        help="Link frequency in Hz (default: 24e9 = 24 GHz)",
    )
    parser.add_argument(
        "--db",
        default="data/nyc_dob.db",
        metavar="PATH",
        help="SQLite database path (default: data/nyc_dob.db)",
    )
    parser.add_argument(
        "--tile-dir",
        default="data/preprocessed",
        metavar="DIR",
        help="Preprocessed LiDAR tile directory (default: data/preprocessed)",
    )
    parser.add_argument(
        "--out-dir",
        default="data/building_heightmaps",
        metavar="DIR",
        help="Output directory (default: data/building_heightmaps)",
    )
    parser.add_argument(
        "--sample-spacing",
        type=int,
        default=5,
        metavar="FEET",
        help="XY sample grid spacing in integer feet (default: 5)",
    )
    parser.add_argument(
        "--mast-offset",
        type=float,
        default=0.0,
        metavar="FEET",
        help=(
            "Vertical offset in feet for the measurement position of the top "
            "point at each (X, Y) location (default: 0)"
        ),
    )
    parser.add_argument(
        "--obs-cache",
        default="data/obstructions",
        metavar="DIR",
        help="Obstruction cache directory (default: data/obstructions)",
    )
    parser.add_argument(
        "--export-obj",
        action="store_true",
        help="Write Minecraft-style OBJ files for terrain and sample points",
    )
    args = parser.parse_args()

    if args.sample_spacing < 1:
        parser.error("--sample-spacing must be >= 1")

    db_path = Path(args.db)
    tile_dir = Path(args.tile_dir)
    out_dir = Path(args.out_dir)
    obs_dir = Path(args.obs_cache) if args.obs_cache else None

    lat, lon, alt_m = args.input_point
    input_point_gps = (lat, lon, alt_m)

    result = run_evaluation(
        args.bin,
        db_path,
        tile_dir,
        out_dir,
        input_point_gps=input_point_gps,
        frequency_hz=args.frequency_hz,
        sample_spacing=args.sample_spacing,
        mast_offset=args.mast_offset,
        obstruction_dir=obs_dir,
        export_obj=args.export_obj,
    )

    print(f"Heightmap:   {result.heightmap_path}")
    print(f"Mask:        {result.mask_path}")
    if result.sample_pts_path:
        pts = np.load(str(result.sample_pts_path))
        print(f"Display pts: {result.sample_pts_path}  ({len(pts)} points)")
        print(f"Meas. pts:   {result.sample_pts_measurement_path}")
    if result.evaluation_path:
        print(f"Evaluations: {result.evaluation_path}")
    if result.viewer_path:
        print(f"Viewer:      {result.viewer_path}")
    if result.terrain_obj_path:
        print(f"Terrain OBJ: {result.terrain_obj_path}")
    for status, path in sorted(result.sample_pts_display_obj_paths.items()):
        print(f"Display OBJ [{status}]: {path}")
    for status, path in sorted(result.sample_pts_measurement_obj_paths.items()):
        print(f"Meas. OBJ   [{status}]: {path}")


if __name__ == "__main__":
    main()
