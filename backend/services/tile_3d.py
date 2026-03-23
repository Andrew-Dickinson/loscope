"""Tile 3D service.

Generates on-demand 3D assets for a single tile:
- Heightmap PNG (for displacement-mapped terrain in Three.js)
- Ortho texture JPEG
- Fresnel zone OBJ
- Obstruction OBJs (one per overlapping obstruction)

Combines logic from tools/run_part2.py (export_zone_obj, export_heightmap)
and tools/tile_to_obj.py (_write_obstruction_obj).
"""
from __future__ import annotations

import io
import json
from pathlib import Path
from typing import Callable

import numpy as np
import tifffile
from PIL import Image

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset

import base64


# ── Shared utilities ──────────────────────────────────────────────────────────

def _tile_sw_corner_nys(tile_id: str) -> tuple[int, int] | None:
    parts = tile_id.rsplit("_", 1)
    if len(parts) != 2 or len(parts[1]) != 2:
        return None
    try:
        xi, yi = int(parts[1][0]), int(parts[1][1])
    except ValueError:
        return None
    origin = file_id_to_offset(parts[0])
    return origin[0] + xi * TILE_SIDE_USFT, origin[1] + yi * TILE_SIDE_USFT


def _export_heightmap_data(tile_id: str, tile_dir: Path) -> dict | None:
    """Return {url, min_height_in, max_height_in} or None."""
    tif_path = tile_dir / f"{tile_id}.tif"
    if not tif_path.exists():
        return None
    raster = tifffile.imread(str(tif_path))
    min_h = int(raster.min())
    max_h = int(raster.max())
    if max_h == 0:
        return None
    raster_img = raster.T[::-1, :]  # (H, W), row 0 = north
    rng = max(max_h - min_h, 1)
    scaled = ((raster_img.astype(np.float32) - min_h) / rng * 255).clip(0, 255).astype(np.uint8)
    buf = io.BytesIO()
    Image.fromarray(scaled, mode="L").save(buf, format="PNG")
    url = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode("ascii")
    return {"url": url, "min_height_in": min_h, "max_height_in": max_h}


def _build_ortho_texture(tile_id: str, ortho_dir: Path) -> str | None:
    """Return JPEG data URI for tile ortho crop, or None."""
    try:
        import imagecodecs
    except ImportError:
        return None
    parts = tile_id.rsplit("_", 1)
    if len(parts) != 2 or len(parts[1]) != 2:
        return None
    file_id = parts[0]
    try:
        xi, yi = int(parts[1][0]), int(parts[1][1])
    except (ValueError, IndexError):
        return None
    jp2_path = ortho_dir / f"{file_id.zfill(6)}.jp2"
    if not jp2_path.exists():
        return None
    try:
        img_arr = imagecodecs.jpeg2k_decode(jp2_path.read_bytes())
    except Exception:
        return None
    row0 = (4 - yi) * 1000
    col0 = xi * 1000
    crop = img_arr[row0:row0 + 1000, col0:col0 + 1000, :3]
    buf = io.BytesIO()
    Image.fromarray(crop.astype(np.uint8), mode="RGB").save(buf, format="JPEG", quality=85)
    return "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode("ascii")


def _export_zone_obj(zone, tile_id: str, out_dir: Path) -> bool:
    """Write Fresnel zone OBJ for one tile. Returns True if written."""
    sw = _tile_sw_corner_nys(tile_id)
    if sw is None:
        return False
    tile_e, tile_n = sw
    S = TILE_SIDE_USFT

    top_z = np.zeros((S, S), dtype=np.float32)
    bot_z = np.zeros((S, S), dtype=np.float32)
    in_zone = np.zeros((S, S), dtype=bool)

    H = int(zone.widths.shape[0])
    for i in range(H):
        w = int(zone.widths[i])
        if w == 0:
            continue
        n = zone.y_base_offset + i
        yi = n - tile_n
        if not (0 <= yi < S):
            continue
        e_row_start = zone.x_base_offset + int(zone.offsets[i])
        j_lo = max(0, tile_e - e_row_start)
        j_hi = min(w, tile_e + S - e_row_start)
        if j_lo >= j_hi:
            continue
        xi_lo = e_row_start + j_lo - tile_e
        xi_hi = e_row_start + j_hi - tile_e
        top_z[xi_lo:xi_hi, yi] = zone.top[i, j_lo:j_hi] / 12.0
        bot_z[xi_lo:xi_hi, yi] = zone.bottom[i, j_lo:j_hi] / 12.0
        in_zone[xi_lo:xi_hi, yi] = True

    if not in_zone.any():
        return False

    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / "zone.obj"
    vi = 1

    with open(out_path, "w") as f:
        f.write(f"# Fresnel zone volume mesh — tile {tile_id}\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n\n")
        f.write(f"o zone_{tile_id.replace('-', '_')}\n\n")

        for xi in range(S):
            for yi in range(S):
                if not in_zone[xi, yi]:
                    continue
                zt = float(top_z[xi, yi])
                zb = float(bot_z[xi, yi])
                x0, y0 = float(xi), float(yi)
                x1, y1 = x0 + 1.0, y0 + 1.0

                # Top face
                f.write(f"v {x0} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y1} {zt:.3f}\n")
                f.write(f"v {x0} {y1} {zt:.3f}\n")
                o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4

                # Bottom face (reversed winding)
                f.write(f"v {x0} {y1} {zb:.3f}\n")
                f.write(f"v {x1} {y1} {zb:.3f}\n")
                f.write(f"v {x1} {y0} {zb:.3f}\n")
                f.write(f"v {x0} {y0} {zb:.3f}\n")
                o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4

                for dxi, dyi, ax, ay, bx, by in (
                    ( 0, -1, x0, y0, x1, y0),
                    ( 0, +1, x1, y1, x0, y1),
                    (+1,  0, x1, y0, x1, y1),
                    (-1,  0, x0, y1, x0, y0),
                ):
                    nxi, nyi = xi + dxi, yi + dyi
                    if 0 <= nxi < S and 0 <= nyi < S and in_zone[nxi, nyi]:
                        nzt = float(top_z[nxi, nyi])
                        nzb = float(bot_z[nxi, nyi])
                        if zt > nzt:
                            f.write(f"v {ax} {ay} {nzt:.3f}\nv {bx} {by} {nzt:.3f}\nv {bx} {by} {zt:.3f}\nv {ax} {ay} {zt:.3f}\n")
                            o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4
                        if zb < nzb:
                            f.write(f"v {ax} {ay} {zb:.3f}\nv {bx} {by} {zb:.3f}\nv {bx} {by} {nzb:.3f}\nv {ax} {ay} {nzb:.3f}\n")
                            o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4
                    else:
                        f.write(f"v {ax} {ay} {zb:.3f}\nv {bx} {by} {zb:.3f}\nv {bx} {by} {zt:.3f}\nv {ax} {ay} {zt:.3f}\n")
                        o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4
    return True


def _write_obstruction_obj(
    obs_id: str,
    meta: dict,
    raster: np.ndarray,
    tile_x_offset: int,
    tile_y_offset: int,
    out_path: Path,
) -> bool:
    """Write one obstruction as an OBJ file. Returns True if non-empty."""
    local_x = int(meta["x_offset"]) - tile_x_offset
    local_y = int(meta["y_offset"]) - tile_y_offset
    W, H = raster.shape
    vi = 1
    cell_count = 0

    xi_lo = max(0, -local_x)
    xi_hi = min(W, TILE_SIDE_USFT - local_x)
    yi_lo = max(0, -local_y)
    yi_hi = min(H, TILE_SIDE_USFT - local_y)

    with open(out_path, "w") as f:
        f.write(f"# Obstruction {obs_id}\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n\n")
        obj_name = f"obs_{obs_id.replace('-', '_')}"
        f.write(f"o {obj_name}\n\n")

        for xi in range(xi_lo, xi_hi):
            for yi in range(yi_lo, yi_hi):
                val = int(raster[xi, yi])
                if val == 0:
                    continue
                zt = val / 12.0
                x0 = float(local_x + xi)
                y0 = float(local_y + yi)
                x1, y1 = x0 + 1.0, y0 + 1.0

                f.write(f"v {x0} {y0} {zt:.3f}\nv {x1} {y0} {zt:.3f}\nv {x1} {y1} {zt:.3f}\nv {x0} {y1} {zt:.3f}\n")
                o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4

                for dxi, dyi, ax, ay, bx, by in [
                    ( 0, -1, x0, y0, x1, y0),
                    ( 0, +1, x1, y1, x0, y1),
                    (+1,  0, x1, y0, x1, y1),
                    (-1,  0, x0, y1, x0, y0),
                ]:
                    nxi2, nyi2 = xi + dxi, yi + dyi
                    if xi_lo <= nxi2 < xi_hi and yi_lo <= nyi2 < yi_hi:
                        nval = int(raster[nxi2, nyi2])
                        nz = nval / 12.0 if nval > 0 else 0.0
                    else:
                        nz = 0.0
                    if zt > nz:
                        f.write(f"v {ax} {ay} {nz:.3f}\nv {bx} {by} {nz:.3f}\nv {bx} {by} {zt:.3f}\nv {ax} {ay} {zt:.3f}\n")
                        o = vi; f.write(f"f {o} {o+1} {o+2} {o+3}\n"); vi += 4
                cell_count += 1

    if cell_count == 0:
        out_path.unlink(missing_ok=True)
        return False
    return True


# ── Main service ──────────────────────────────────────────────────────────────

def run_tile_3d_service(
    tile_id: str,
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    frequency_hz: float,
    progress_cb: Callable[[int, str], None],
    tile_dir: Path,
    obstruction_dir: Path,
    ortho_dir: Path,
    cache_dir: Path,
    alpha: float = 1.0,
) -> dict:
    from uuid import uuid4
    job_id = uuid4().hex

    out_dir = cache_dir / "tile_3d" / job_id
    out_dir.mkdir(parents=True, exist_ok=True)

    progress_cb(10, "Loading heightmap…")
    heightmap = _export_heightmap_data(tile_id, tile_dir)

    progress_cb(20, "Loading ortho texture…")
    ortho_texture = _build_ortho_texture(tile_id, ortho_dir)

    progress_cb(35, "Computing Fresnel zone…")
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)

    progress_cb(55, "Generating zone OBJ…")
    zone_obj_available = _export_zone_obj(zone, tile_id, out_dir)

    progress_cb(65, "Generating obstruction OBJs…")
    sw = _tile_sw_corner_nys(tile_id)
    tile_x_offset, tile_y_offset = (sw[0], sw[1]) if sw else (0, 0)

    obstruction_ids: list[str] = []
    obs_info: dict[str, dict] = {}

    for json_path in sorted(obstruction_dir.glob("*.json")):
        try:
            meta = json.loads(json_path.read_text())
        except (ValueError, OSError):
            continue
        if tile_id not in meta.get("tile_ids", []):
            continue
        obs_id = meta["obstruction_id"]
        tif_path = obstruction_dir / meta.get("raster_file", f"{obs_id}.tif")
        try:
            raster = tifffile.imread(str(tif_path))
        except OSError:
            continue
        out_path = out_dir / f"{obs_id}.obj"
        if _write_obstruction_obj(obs_id, meta, raster, tile_x_offset, tile_y_offset, out_path):
            obstruction_ids.append(obs_id)
            obs_info[obs_id] = {
                "type": meta.get("obstruction_type", "unknown"),
                "attributes": meta.get("attributes", {}),
            }

    progress_cb(90, "Done")
    return {
        "tile_id": tile_id,
        "job_id": job_id,
        "heightmap": heightmap,
        "ortho_texture": ortho_texture,
        "zone_obj_available": zone_obj_available,
        "obstruction_ids": obstruction_ids,
        "obs_info": obs_info,
    }
