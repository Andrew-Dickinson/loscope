"""Tile map service.

Extracts the data computation from tools/run_part2.py, returning a
JSON-serializable dict for consumption by the React frontend.
"""
from __future__ import annotations

import base64
import io
import json
import math
import os
from pathlib import Path
from typing import Callable

import numpy as np
import tifffile
from PIL import Image
from pyproj import Transformer

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone, translate_to_nys_plane
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset
from los_analyzer.tiles.identify import identify_tiles
from los_analyzer.tiles.intersect import compute_intersection
from los_analyzer.tiles.load import load_terrain_grid

_TO_WGS84 = Transformer.from_crs("EPSG:6539", "EPSG:4326", always_xy=True)
_C_USFT_PER_S = 299_792_458 / 0.3048006096


# ── Utilities (ported from run_part2.py) ─────────────────────────────────────

def _lonlat(easting: float, northing: float) -> list[float]:
    lon, lat = _TO_WGS84.transform(easting, northing)
    return [lon, lat]


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


def _fresnel_ellipse_ring(
    nys_a: tuple,
    nys_b: tuple,
    frequency_hz: float,
    alpha: float = 1.0,
    n_pts: int = 90,
) -> list[list[float]]:
    cx = (nys_a[0] + nys_b[0]) / 2
    cy = (nys_a[1] + nys_b[1]) / 2
    dx = nys_b[0] - nys_a[0]
    dy = nys_b[1] - nys_a[1]
    L = math.sqrt(dx**2 + dy**2)
    if L == 0:
        return []
    theta = math.atan2(dy, dx)
    semi_major = L / 2
    wavelength_usft = _C_USFT_PER_S / frequency_hz
    semi_minor = alpha * math.sqrt(wavelength_usft * L / 4)
    ring = []
    for i in range(n_pts + 1):
        t = 2 * math.pi * i / n_pts
        xl = semi_major * math.cos(t)
        yl = semi_minor * math.sin(t)
        e = cx + xl * math.cos(theta) - yl * math.sin(theta)
        n = cy + xl * math.sin(theta) + yl * math.cos(theta)
        ring.append(_lonlat(e, n))
    return ring


def _tile_obstruction_overlay(
    tile_id: str,
    obstruction,
) -> tuple[str, list] | tuple[None, None]:
    sw = _tile_sw_corner_nys(tile_id)
    if sw is None:
        return None, None
    e0, n0 = sw

    H = int(obstruction.widths.shape[0])
    i_start = max(0, n0 - obstruction.y_base_offset)
    i_end = min(H, n0 + TILE_SIDE_USFT - obstruction.y_base_offset)
    if i_start >= i_end:
        return None, None

    img_h = i_end - i_start
    rgba = np.zeros((img_h, TILE_SIDE_USFT, 4), dtype=np.uint8)
    has_obstruction = False

    for i in range(i_start, i_end):
        width = int(obstruction.widths[i])
        if width == 0:
            continue
        e_row_start = obstruction.x_base_offset + int(obstruction.offsets[i])
        overlap_e_start = max(e_row_start, e0)
        overlap_e_end = min(e_row_start + width, e0 + TILE_SIDE_USFT)
        if overlap_e_start >= overlap_e_end:
            continue
        col_start = overlap_e_start - e_row_start
        col_end = overlap_e_end - e_row_start
        vals = obstruction.values[i, col_start:col_end].clip(0.0, 1.0)
        nonzero = vals > 0
        if not nonzero.any():
            continue
        has_obstruction = True
        img_col = overlap_e_start - e0
        img_row = i - i_start
        r = np.where(vals <= 0.5, vals * 2.0 * 255, 255).astype(np.uint8)
        g = np.where(vals <= 0.5, 255, (1.0 - (vals - 0.5) * 2.0) * 255).astype(np.uint8)
        b = np.zeros(len(vals), dtype=np.uint8)
        a = np.where(nonzero, 200, 0).astype(np.uint8)
        rgba[img_row, img_col:img_col + len(vals)] = np.stack([r, g, b, a], axis=1)

    if not has_obstruction:
        return None, None

    rgba = rgba[::-1]
    buf = io.BytesIO()
    Image.fromarray(rgba, "RGBA").save(buf, format="PNG")
    data_url = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode("ascii")
    actual_n0 = obstruction.y_base_offset + i_start
    actual_n1 = obstruction.y_base_offset + i_end
    sw_ll = _lonlat(e0, actual_n0)
    ne_ll = _lonlat(e0 + TILE_SIDE_USFT, actual_n1)
    bounds = [[sw_ll[1], sw_ll[0]], [ne_ll[1], ne_ll[0]]]
    return data_url, bounds


def _obs_raster_b64(tif_path: Path) -> str | None:
    arr = tifffile.imread(str(tif_path)).astype(np.float32)
    max_val = arr.max()
    if max_val == 0:
        return None
    scaled = np.clip(arr / max_val * 255, 0, 255).astype(np.uint8)
    img_arr = scaled.T[::-1, :]
    buf = io.BytesIO()
    Image.fromarray(img_arr, mode="L").save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode("ascii")


def _build_ortho_textures(tile_ids: list[str], ortho_dir: Path) -> dict[str, str]:
    try:
        import imagecodecs
    except ImportError:
        return {}
    from collections import defaultdict

    by_file: dict[str, list[str]] = defaultdict(list)
    for tid in tile_ids:
        parts = tid.rsplit("_", 1)
        if len(parts) == 2 and len(parts[1]) == 2:
            by_file[parts[0]].append(tid)

    textures: dict[str, str] = {}
    for file_id, tids in by_file.items():
        jp2_path = ortho_dir / f"{file_id.zfill(6)}.jp2"
        if not jp2_path.exists():
            continue
        try:
            img_arr = imagecodecs.jpeg2k_decode(jp2_path.read_bytes())
        except Exception as e:
            print(f"  Warning: could not decode ortho {jp2_path.name}: {e}")
            continue
        for tid in tids:
            try:
                xi = int(tid[-2])
                yi = int(tid[-1])
            except (ValueError, IndexError):
                continue
            row0 = (4 - yi) * 1000
            col0 = xi * 1000
            crop = img_arr[row0:row0 + 1000, col0:col0 + 1000, :3]
            buf = io.BytesIO()
            Image.fromarray(crop.astype(np.uint8), mode="RGB").save(buf, format="JPEG", quality=85)
            textures[tid] = "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode("ascii")
    return textures


def export_heightmap_data(tile_id: str, tile_dir: Path) -> dict | None:
    """Return {url, min_height_in, max_height_in} for a tile, or None."""
    tif_path = tile_dir / f"{tile_id}.tif"
    if not tif_path.exists():
        return None
    raster = tifffile.imread(str(tif_path))
    min_h = int(raster.min())
    max_h = int(raster.max())
    if max_h == 0:
        return None
    raster_img = raster.T[::-1, :]
    rng = max(max_h - min_h, 1)
    scaled = ((raster_img.astype(np.float32) - min_h) / rng * 255).clip(0, 255).astype(np.uint8)
    buf = io.BytesIO()
    Image.fromarray(scaled, mode="L").save(buf, format="PNG")
    url = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode("ascii")
    return {"url": url, "min_height_in": min_h, "max_height_in": max_h}


# ── Main service ──────────────────────────────────────────────────────────────

def run_tile_map_service(
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    frequency_hz: float,
    progress_cb: Callable[[int, str], None],
    tile_dir: Path,
    obstruction_dir: Path,
    ortho_dir: Path,
    alpha: float = 1.0,
) -> dict:
    """Compute tile map data for a given LOS pair.

    Returns a JSON-serializable dict ready for the React TileMap component.
    """
    progress_cb(5, "Computing Fresnel zone…")
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)

    progress_cb(15, "Identifying tiles…")
    tiles = identify_tiles(zone, tile_dir)

    progress_cb(25, "Loading terrain grid…")
    terrain = load_terrain_grid(zone, tiles, tile_dir, obstruction_types="*",
                                obstruction_dir=obstruction_dir)

    progress_cb(45, "Computing obstruction intersection…")
    obstruction = compute_intersection(zone, terrain)

    progress_cb(55, "Building tile overlays…")
    obstructed_tile_ids: set[str] = set()
    tile_overlays: list[dict] = []
    for tid in tiles:
        url, bounds = _tile_obstruction_overlay(tid, obstruction)
        if url is not None:
            tile_overlays.append({"url": url, "bounds": bounds})
            obstructed_tile_ids.add(tid)

    # Build obstruction metadata
    tile_obs_info: dict[str, list[dict]] = {}
    obs_rasters: dict[str, str] = {}
    for obs_id in terrain.matched_obstruction_ids:
        json_path = obstruction_dir / f"{obs_id}.json"
        try:
            meta = json.loads(json_path.read_text())
        except (FileNotFoundError, ValueError):
            continue
        entry = {
            "id": obs_id,
            "type": meta.get("obstruction_type", "unknown"),
            "attributes": meta.get("attributes", {}),
        }
        for tid in meta.get("tile_ids", []):
            tile_obs_info.setdefault(tid, []).append(entry)
        tif_path = obstruction_dir / meta.get("raster_file", f"{obs_id}.tif")
        b64 = _obs_raster_b64(tif_path)
        if b64 is not None:
            obs_rasters[obs_id] = b64

    # Tile GeoJSON features
    tile_features = []
    for tile_id in tiles:
        sw = _tile_sw_corner_nys(tile_id)
        if sw is None:
            continue
        e0, n0 = sw
        e1, n1 = e0 + TILE_SIDE_USFT, n0 + TILE_SIDE_USFT
        ring = [
            _lonlat(e0, n0), _lonlat(e1, n0), _lonlat(e1, n1),
            _lonlat(e0, n1), _lonlat(e0, n0),
        ]
        tile_features.append({
            "type": "Feature",
            "properties": {
                "id": tile_id,
                "hasObstruction": tile_id in obstructed_tile_ids,
                "obstructions": tile_obs_info.get(tile_id, []),
            },
            "geometry": {"type": "Polygon", "coordinates": [ring]},
        })

    # LOS line + endpoints
    a_ll = _lonlat(nys_a[0], nys_a[1])
    b_ll = _lonlat(nys_b[0], nys_b[1])
    los_features = [
        {"type": "Feature", "properties": {},
         "geometry": {"type": "LineString", "coordinates": [a_ll, b_ll]}},
        {"type": "Feature", "properties": {"label": "A"},
         "geometry": {"type": "Point", "coordinates": a_ll}},
        {"type": "Feature", "properties": {"label": "B"},
         "geometry": {"type": "Point", "coordinates": b_ll}},
    ]

    # Fresnel ellipse (plan view)
    ellipse_ring = _fresnel_ellipse_ring(nys_a, nys_b, frequency_hz, alpha)
    fresnel_ellipse = {
        "type": "Feature",
        "properties": {},
        "geometry": {"type": "Polygon", "coordinates": [ellipse_ring]},
    } if ellipse_ring else None

    progress_cb(70, "Building heightmaps…")
    tile_heightmaps: dict[str, dict] = {}
    for tid in tiles:
        hm = export_heightmap_data(tid, tile_dir)
        if hm:
            tile_heightmaps[tid] = hm

    progress_cb(85, "Building ortho textures…")
    tile_ortho_textures = _build_ortho_textures(tiles, ortho_dir)

    progress_cb(95, "Done")
    return {
        "tiles": {"type": "FeatureCollection", "features": tile_features},
        "tile_overlays": tile_overlays,
        "fresnel_ellipse": fresnel_ellipse,
        "los_line": {"type": "FeatureCollection", "features": los_features},
        "tile_heightmaps": tile_heightmaps,
        "tile_ortho_textures": tile_ortho_textures,
        "obs_rasters": obs_rasters,
        "tile_obs_info": tile_obs_info,
        # Pass back nys coords so frontend can use them for tile-3d requests
        "nys_a": list(nys_a),
        "nys_b": list(nys_b),
        "frequency_ghz": frequency_hz / 1e9,
    }
