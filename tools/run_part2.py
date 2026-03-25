"""
Run Part 2 obstruction-detection steps 2.1 through 2.4.

Usage:
    python tools/run_part2.py [tile_dir]

Default tile_dir: data/preprocessed
Writes an interactive HTML tile map to data/tile_map.html.
"""
import base64
import io
import json
import math
import os
import sys
from pathlib import Path

import imagecodecs
import numpy as np
import tifffile
from PIL import Image
from pyproj import Transformer

from lib.fresnel.fresnel_zone2 import compute_fresnel_zone, translate_to_nys_plane
from lib.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset
from lib.tiles.identify import identify_tiles
from lib.tiles.intersect import compute_intersection
from lib.tiles.load import load_terrain_grid

GPS_A = ( 40.841668, -73.941836, 141)
GPS_B = ( 40.843877, -73.893044, 69.0)
FREQUENCY_HZ = 24_000_000_000
ALPHA = 1.0

_TO_WGS84 = Transformer.from_crs("EPSG:6539", "EPSG:4326", always_xy=True)

# Speed of light in US survey feet per second
_C_USFT_PER_S = 299_792_458 / 0.3048006096


def _fresnel_ellipse_ring(
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    frequency_hz: float,
    alpha: float,
    n_pts: int = 90,
) -> list[list[float]]:
    """Return a GeoJSON ring [lon, lat] tracing the overhead Fresnel ellipse.

    Uses the analytic formula for the first Fresnel zone radius at the midpoint
    (where it is maximum): r = sqrt(lambda * L / 4), scaled by alpha.
    The ellipse semi-major axis equals half the 2-D horizontal LOS length.
    Height is ignored (overhead plan-view projection).
    """
    cx = (nys_a[0] + nys_b[0]) / 2
    cy = (nys_a[1] + nys_b[1]) / 2
    dx = nys_b[0] - nys_a[0]
    dy = nys_b[1] - nys_a[1]
    L = math.sqrt(dx ** 2 + dy ** 2)
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


def _tile_sw_corner_nys(tile_id: str) -> tuple[int, int] | None:
    """Return (easting, northing) in usft for the SW corner of tile_id."""
    parts = tile_id.rsplit("_", 1)
    if len(parts) != 2 or len(parts[1]) != 2:
        return None
    try:
        xi, yi = int(parts[1][0]), int(parts[1][1])
    except ValueError:
        return None
    origin = file_id_to_offset(parts[0])
    return origin[0] + xi * TILE_SIDE_USFT, origin[1] + yi * TILE_SIDE_USFT


def _lonlat(easting: float, northing: float) -> list[float]:
    lon, lat = _TO_WGS84.transform(easting, northing)
    return [lon, lat]


def _tile_obstruction_overlay(
    tile_id: str,
    obstruction,
) -> tuple[str, list] | tuple[None, None]:
    """Render the obstruction values for a single tile as a base64 PNG.

    Color scale: green (0) → yellow (0.5) → red (1.0).  Pixels with
    obstruction == 0 are fully transparent so only obstructed cells are drawn.
    Returns (None, None) when the tile has no overlap with the obstruction grid
    or all overlapping values are zero.
    """
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

    rgba = rgba[::-1]  # flip: image row 0 = northernmost

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
    """Load an obstruction tiff and return a grayscale PNG scaled by its max value."""
    arr = tifffile.imread(str(tif_path)).astype(np.float32)
    max_val = arr.max()
    if max_val == 0:
        return None
    scaled = np.clip(arr / max_val * 255, 0, 255).astype(np.uint8)
    img_arr = scaled.T[::-1, :]  # transpose to (H, W), flip so row 0 = north
    buf = io.BytesIO()
    Image.fromarray(img_arr, mode="L").save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode("ascii")


def _build_ortho_textures(tile_ids: list[str], ortho_dir: Path) -> dict[str, str]:
    """Crop ortho JP2s to per-sub-tile JPEG data URIs for embedding in HTML.

    Ortho JP2 layout:
      - One file per LAS tile: <ortho_dir>/<file_id.zfill(6)>.jp2
      - Covers the full 2500×2500 usft LAS tile at 5000×5000 pixels (2 px/usft)
      - Row 0 = north (highest northing), row 4999 = south (lowest northing)
      - Col 0 = west (lowest easting),    col 4999 = east (highest easting)

    Sub-tile (xi, yi) maps to pixel region:
      rows [4000 - yi*1000 : 5000 - yi*1000]  (yi=0 → rows 4000-4999 = south)
      cols [xi*1000        : xi*1000 + 1000]   (xi=0 → cols 0-999 = west)

    Each 1000×1000 crop is JPEG-encoded at quality 85 and returned as a
    data URI.  JP2 files that are absent from ortho_dir are silently skipped.
    Each JP2 is decoded once regardless of how many sub-tiles it contributes.
    """
    from collections import defaultdict

    # Group sub-tiles by their parent LAS file_id.
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
            # Decode full JP2 once; shape (5000, 5000, bands) uint8
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
            crop = img_arr[row0:row0 + 1000, col0:col0 + 1000, :3]  # RGB only
            buf = io.BytesIO()
            Image.fromarray(crop.astype(np.uint8), mode="RGB").save(buf, format="JPEG", quality=85)
            textures[tid] = "data:image/jpeg;base64," + base64.b64encode(buf.getvalue()).decode("ascii")

    return textures


def export_heightmap(tile_id: str, tile_dir: Path, out_dir: Path) -> bool:
    """Export a normalized 8-bit grayscale PNG heightmap for a tile.

    Raster axes are [easting, northing] (uint16, values in inches).
    The PNG is saved with row 0 = north so Three.js (flipY=true default)
    maps UV.v=1 → north and UV.v=0 → south, consistent with a PlaneGeometry
    rotated -PI/2 around X.

    A companion JSON is written with min_height_in and max_height_in so the
    browser can compute the correct displacement scale.
    Returns True if the tile was found and exported successfully.
    """
    tif_path = tile_dir / f"{tile_id}.tif"
    if not tif_path.exists():
        return False

    raster = tifffile.imread(str(tif_path))  # uint16, shape (500, 500), [easting, northing]
    min_h = int(raster.min())
    max_h = int(raster.max())
    if max_h == 0:
        return False

    # Transpose to [northing, easting], flip so row 0 = north (highest northing).
    raster_img = raster.T[::-1, :]  # shape (500, 500)
    rng = max(max_h - min_h, 1)
    scaled = ((raster_img.astype(np.float32) - min_h) / rng * 255).clip(0, 255).astype(np.uint8)

    out_dir.mkdir(parents=True, exist_ok=True)
    Image.fromarray(scaled, mode="L").save(out_dir / f"{tile_id}.png")
    (out_dir / f"{tile_id}.json").write_text(
        json.dumps({"min_height_in": min_h, "max_height_in": max_h})
    )
    return True


def generate_tile_map(
    tile_ids: list[str],
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    output_path: Path,
    frequency_hz: float = FREQUENCY_HZ,
    alpha: float = ALPHA,
    obstruction=None,
    tile_obs_info: dict | None = None,
    obs_rasters: dict | None = None,
    tile_heightmaps: dict | None = None,
    tile_ortho_textures: dict | None = None,
) -> None:
    """Write an interactive split-view Leaflet + Three.js HTML map to output_path.

    Left panel (~60%): Leaflet 2D tile map with obstruction overlays.
    Right panel (~40%): Three.js 3D view opened by clicking any tile.
      - Terrain rendered as PlaneGeometry with displacement map from heightmap PNG.
      - Fresnel zone OBJ (obj-zone/{tile_id}_zone.obj) loaded as translucent mesh.
      - Obstruction OBJs (obj/{tile_id}_{obs_id}.obj) loaded as pickable colored meshes.
      - OrbitControls for mouse/touch spin and zoom.
      - Pointer-click raycasting shows obstruction metadata in overlay label.

    tile_heightmaps: dict mapping tile_id to
        {"url": "data:image/png;base64,...", "min_height_in": int, "max_height_in": int}
    Heightmap data is embedded directly in the HTML so it works from file:// without a server.
    """
    # Build per-tile obstruction overlays first so we know which tiles are obstructed
    # before constructing tile features (needed for styling).
    obstructed_tile_ids: set[str] = set()
    tile_overlays: list[dict] = []
    if obstruction is not None:
        for tid in tile_ids:
            url, bounds = _tile_obstruction_overlay(tid, obstruction)
            if url is not None:
                tile_overlays.append({"url": url, "bounds": bounds})
                obstructed_tile_ids.add(tid)

    tile_features = []
    for tile_id in tile_ids:
        sw = _tile_sw_corner_nys(tile_id)
        if sw is None:
            continue
        e0, n0 = sw
        e1, n1 = e0 + TILE_SIDE_USFT, n0 + TILE_SIDE_USFT
        ring = [
            _lonlat(e0, n0),
            _lonlat(e1, n0),
            _lonlat(e1, n1),
            _lonlat(e0, n1),
            _lonlat(e0, n0),
        ]
        tile_features.append({
            "type": "Feature",
            "properties": {
                "id": tile_id,
                "hasObstruction": tile_id in obstructed_tile_ids,
                "obstructions": (tile_obs_info or {}).get(tile_id, []),
            },
            "geometry": {"type": "Polygon", "coordinates": [ring]},
        })

    a_ll = _lonlat(nys_a[0], nys_a[1])
    b_ll = _lonlat(nys_b[0], nys_b[1])
    los_features = [
        {
            "type": "Feature",
            "properties": {},
            "geometry": {"type": "LineString", "coordinates": [a_ll, b_ll]},
        },
        {
            "type": "Feature",
            "properties": {"label": "A"},
            "geometry": {"type": "Point", "coordinates": a_ll},
        },
        {
            "type": "Feature",
            "properties": {"label": "B"},
            "geometry": {"type": "Point", "coordinates": b_ll},
        },
    ]

    ellipse_ring = _fresnel_ellipse_ring(nys_a, nys_b, frequency_hz, alpha)
    ellipse_js = json.dumps({
        "type": "Feature",
        "properties": {},
        "geometry": {"type": "Polygon", "coordinates": [ellipse_ring]},
    }) if ellipse_ring else "null"

    tiles_js = json.dumps({"type": "FeatureCollection", "features": tile_features})
    los_js = json.dumps({"type": "FeatureCollection", "features": los_features})
    tile_overlays_js = json.dumps(tile_overlays)
    obs_rasters_js = json.dumps(obs_rasters or {})
    tile_heightmaps_js = json.dumps(tile_heightmaps or {})
    tile_ortho_textures_js = json.dumps(tile_ortho_textures or {})

    # Fallback center (midpoint of LOS line in lat/lon) if no tiles are present.
    cx = (a_ll[1] + b_ll[1]) / 2
    cy = (a_ll[0] + b_ll[0]) / 2

    html = f"""\
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>LOS Tile Map</title>
<link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"/>
<script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
<script type="importmap">{{
  "imports": {{
    "three": "https://cdn.jsdelivr.net/npm/three@0.169/build/three.module.js",
    "three/addons/": "https://cdn.jsdelivr.net/npm/three@0.169/examples/jsm/"
  }}
}}</script>
<style>
  * {{ box-sizing: border-box; }}
  html, body {{ height: 100%; margin: 0; padding: 0; display: flex; flex-direction: column; overflow: hidden; }}
  #main {{ display: flex; flex: 1; min-height: 0; }}
  #map {{ flex: 0 0 60%; height: 100%; }}
  #panel-3d {{
    flex: 0 0 40%; height: 100%; position: relative;
    background: #0e1117; display: flex; flex-direction: column;
    border-left: 2px solid #222;
  }}
  #panel-3d-header {{
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 10px; background: #181c24; flex-shrink: 0;
    border-bottom: 1px solid #333;
  }}
  #panel-3d-title {{ color: #aaa; font-family: monospace; font-size: 12px; }}
  #panel-3d-close {{
    border: none; background: none; color: #666; font-size: 18px;
    cursor: pointer; line-height: 1; padding: 0 4px;
  }}
  #panel-3d-close:hover {{ color: #ccc; }}
  #panel-3d-body {{ flex: 1; position: relative; min-height: 0; }}
  #placeholder-3d {{
    position: absolute; inset: 0; display: flex;
    align-items: center; justify-content: center;
    color: #444; font-family: sans-serif; font-size: 13px;
    text-align: center; padding: 20px;
  }}
  #canvas-3d {{ display: none; width: 100%; height: 100%; }}
  #label-3d {{
    display: none; position: absolute; bottom: 10px; left: 50%;
    transform: translateX(-50%); background: rgba(0,0,0,0.75);
    color: #fff; font-family: monospace; font-size: 11px;
    padding: 4px 10px; border-radius: 4px; white-space: nowrap;
    max-width: 90%; overflow: hidden; text-overflow: ellipsis;
    pointer-events: none;
  }}
  #loading-3d {{
    display: none; position: absolute; inset: 0;
    align-items: center; justify-content: center;
    background: rgba(14,17,23,0.7);
    color: #888; font-family: sans-serif; font-size: 12px;
  }}
  #loading-3d.active {{ display: flex; }}
  #obs-modal {{
    display: none; position: fixed; inset: 0;
    background: rgba(0,0,0,0.72); z-index: 9999;
    align-items: center; justify-content: center;
  }}
  #obs-modal.open {{ display: flex; }}
  #obs-modal-box {{
    background: #fff; border-radius: 6px; padding: 16px;
    max-width: 90vw; max-height: 90vh; overflow: auto;
  }}
  #obs-modal-header {{
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 10px; gap: 16px;
  }}
  #obs-modal-title {{ font-family: monospace; font-size: 12px; color: #555; }}
  #obs-modal-close {{
    border: none; background: none; font-size: 22px;
    line-height: 1; cursor: pointer; color: #888; padding: 0;
  }}
  #obs-modal-img {{
    display: block; image-rendering: pixelated;
    max-width: 80vw; max-height: 70vh;
    border: 1px solid #ddd;
  }}
  #scene-legend {{
    flex-shrink: 0; max-height: 35%; overflow-y: auto;
    background: #0a0d13; border-top: 1px solid #2a2a3a; padding: 8px 10px;
  }}
  #scene-legend-title {{
    color: #555; font-family: monospace; font-size: 10px;
    text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 6px;
  }}
  .legend-item {{
    display: flex; align-items: center; gap: 8px; padding: 3px 0;
    font-family: monospace; font-size: 11px; color: #bbb;
  }}
  .legend-swatch {{
    width: 11px; height: 11px; border-radius: 2px; flex-shrink: 0;
    border: 1px solid rgba(255,255,255,0.15);
  }}
  .legend-label {{ flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }}
  .legend-toggle {{ cursor: pointer; accent-color: #cc44ff; flex-shrink: 0; }}
  @media (max-width: 700px) {{
    #main {{ flex-direction: column; }}
    #map {{ flex: 0 0 50%; }}
    #panel-3d {{ flex: 0 0 50%; border-left: none; border-top: 2px solid #222; }}
  }}
</style>
</head>
<body>
<div id="main">
  <div id="map"></div>
  <div id="panel-3d">
    <div id="panel-3d-header">
      <span id="panel-3d-title">3D View</span>
      <button id="panel-3d-close" onclick="close3DPanel()" title="Close">&times;</button>
    </div>
    <div id="panel-3d-body">
      <div id="placeholder-3d">Click a tile on the map<br>to open the 3D view</div>
      <canvas id="canvas-3d"></canvas>
      <div id="label-3d"></div>
      <div id="loading-3d"><span id="loading-3d-text">Loading...</span></div>
    </div>
    <div id="scene-legend" style="display:none">
      <div id="scene-legend-title">Scene objects</div>
      <div id="scene-legend-list"></div>
    </div>
  </div>
</div>
<div id="obs-modal" onclick="if(event.target===this)closeObsModal()">
  <div id="obs-modal-box">
    <div id="obs-modal-header">
      <code id="obs-modal-title"></code>
      <button id="obs-modal-close" onclick="closeObsModal()">&times;</button>
    </div>
    <img id="obs-modal-img" src="" alt="obstruction raster"/>
  </div>
</div>

<!-- Leaflet map (non-module) -->
<script>
var tilesData = {tiles_js};
var losData = {los_js};
var ellipseData = {ellipse_js};
var tileOverlays = {tile_overlays_js};
var obsRasters = {obs_rasters_js};
var tileHeightmaps = {tile_heightmaps_js};
var tileOrthoTextures = {tile_ortho_textures_js};

function showObsRaster(id) {{
  var b64 = obsRasters[id];
  if (!b64) return;
  document.getElementById('obs-modal-title').textContent = id;
  document.getElementById('obs-modal-img').src = 'data:image/png;base64,' + b64;
  document.getElementById('obs-modal').classList.add('open');
}}
function closeObsModal() {{
  document.getElementById('obs-modal').classList.remove('open');
  document.getElementById('obs-modal-img').src = '';
}}
function close3DPanel() {{
  document.getElementById('panel-3d').style.display = 'none';
  document.getElementById('map').style.flex = '1';
  setTimeout(function() {{ if (window._leafletMap) window._leafletMap.invalidateSize(); }}, 50);
}}

var map = L.map('map', {{maxZoom: 22}});
window._leafletMap = map;
L.tileLayer('https://{{s}}.tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{
  attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
  maxNativeZoom: 19,
  maxZoom: 22
}}).addTo(map);

tileOverlays.forEach(function(o) {{
  L.imageOverlay(o.url, o.bounds, {{opacity: 0.85, zIndex: 200}}).addTo(map);
}});

if (ellipseData) {{
  L.geoJSON(ellipseData, {{
    style: {{ color: '#ff8800', weight: 2, dashArray: '8 5', fillOpacity: 0.08, fillColor: '#ff8800' }}
  }}).addTo(map);
}}

var tileLayer = L.geoJSON(tilesData, {{
  style: function(f) {{
    return f.properties.hasObstruction
      ? {{ color: '#e63030', weight: 2, fillOpacity: 0.15, fillColor: '#e63030' }}
      : {{ color: '#3388ff', weight: 1, fillOpacity: 0.05, fillColor: '#3388ff' }};
  }},
  onEachFeature: function(f, l) {{
    var p = f.properties;
    var html = '<code>' + p.id + '</code>';
    if (p.hasObstruction) {{
      html += ' <span style="color:#e63030;font-weight:bold">obstructed</span>';
    }}
    var obs = p.obstructions || [];
    if (obs.length > 0) {{
      html += '<br><b>' + obs.length + ' obstruction(s):</b>';
      html += '<div style="max-height:220px;overflow-y:auto;margin-top:4px">';
      html += '<ul style="margin:0;padding-left:16px">';
      obs.forEach(function(o) {{
        var typeLabel = o.type.replace(/_/g, ' ');
        html += '<li style="font-size:11px;margin-bottom:4px">';
        html += '<b>' + typeLabel + '</b>';
        html += ' <span style="color:#888;font-size:10px">' + o.id + '</span>';
        if (obsRasters[o.id]) {{
          html += ' <a href="#" style="font-size:10px" onclick="showObsRaster(\\'' + o.id + '\\');return false">view raster</a>';
        }}
        var a = o.attributes || {{}};
        var keys = Object.keys(a);
        if (keys.length > 0) {{
          html += '<table style="margin:2px 0 0 8px;border-collapse:collapse;font-size:11px">';
          keys.forEach(function(k) {{
            var v = a[k];
            if (v === null || v === '' || v === undefined) return;
            html += '<tr><td style="color:#666;padding-right:6px">' + k + '</td><td>' + v + '</td></tr>';
          }});
          html += '</table>';
        }}
        html += '</li>';
      }});
      html += '</ul></div>';
    }}
    l.bindPopup(html);
    l.on('click', function() {{
      if (window.show3DTile) window.show3DTile(p.id);
    }});
  }}
}}).addTo(map);

L.geoJSON(losData, {{
  style: function(f) {{
    return {{ color: '#e63030', weight: 2, opacity: 0.9 }};
  }},
  pointToLayer: function(f, latlng) {{
    return L.circleMarker(latlng, {{
      radius: 7, color: '#e63030', fillColor: '#e63030', fillOpacity: 1, weight: 2
    }}).bindTooltip(f.properties.label, {{ permanent: true, direction: 'right' }});
  }}
}}).addTo(map);

var losBounds = L.geoJSON(losData).getBounds();
if (losBounds.isValid()) {{
  map.fitBounds(losBounds.pad(0.15));
}} else {{
  map.setView([{cx:.6f}, {cy:.6f}], 15);
}}
</script>

<!-- Three.js 3D panel (ES module) -->
<script type="module">
import * as THREE from 'three';
import {{ OBJLoader }} from 'three/addons/loaders/OBJLoader.js';
import {{ OrbitControls }} from 'three/addons/controls/OrbitControls.js';

let renderer = null;
let scene = null;
let camera = null;
let controls = null;
let animFrameId = null;
const raycaster = new THREE.Raycaster();
const pointer = new THREE.Vector2();
const pickableMeshes = [];
const meshMeta = new Map();
const sceneObjects = [];  // {{ label, color, obj }} — drives the DOM legend

function getPanel()  {{ return document.getElementById('panel-3d-body'); }}
function getCanvas() {{ return document.getElementById('canvas-3d'); }}

function setLoading(text) {{
  const el = document.getElementById('loading-3d');
  const tx = document.getElementById('loading-3d-text');
  if (text) {{ tx.textContent = text; el.classList.add('active'); }}
  else       {{ el.classList.remove('active'); }}
}}

function setLabel(text) {{
  const el = document.getElementById('label-3d');
  el.textContent = text;
  el.style.display = text ? 'block' : 'none';
}}

function initRenderer() {{
  const canvas = getCanvas();
  renderer = new THREE.WebGLRenderer({{ canvas, antialias: true }});
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));


  camera = new THREE.PerspectiveCamera(50, 1, 0.5, 200000);

  controls = new OrbitControls(camera, canvas);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.minDistance = 10;
  controls.maxDistance = 5000;
  controls.maxPolarAngle = Math.PI * 0.9;

  canvas.addEventListener('pointerdown', onCanvasPointerDown);
  window.addEventListener('resize', onWindowResize);

  function animate() {{
    animFrameId = requestAnimationFrame(animate);
    controls.update();
    if (scene && camera) renderer.render(scene, camera);
  }}
  animate();
}}

function onWindowResize() {{
  if (!renderer || !camera) return;
  const panel = getPanel();
  const w = panel.clientWidth, h = panel.clientHeight;
  if (w === 0 || h === 0) return;
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  renderer.setSize(w, h, false);
}}

function disposeScene() {{
  if (!scene) return;
  scene.traverse(obj => {{
    if (obj.geometry) obj.geometry.dispose();
    if (obj.material) {{
      const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
      mats.forEach(m => {{
        if (m.map) m.map.dispose();
        if (m.displacementMap) m.displacementMap.dispose();
        m.dispose();
      }});
    }}
  }});
  scene = null;
}}

function loadOBJ(url) {{
  return new Promise((resolve, reject) => {{
    new OBJLoader().load(url, resolve, undefined, reject);
  }});
}}

// OBJ coordinate system: X=local easting (0-499 ft), Y=local northing (0-499 ft), Z=elevation (ft).
// Three.js world: X=easting, Y=elevation (up), Z=-northing (Z increases southward).
// Transform: rotation.x = -PI/2, position = (-250, 0, 250) centers the tile at world origin.
function applyObjTransform(obj) {{
  obj.rotation.x = -Math.PI / 2;
  obj.position.set(-250, 0, 250);
}}

const OBS_COLORS = [0xff6633, 0xffaa00, 0xff3388, 0xaa44ff, 0x44ccff];

async function loadTile(tileId) {{
  document.getElementById('panel-3d').style.display = '';
  document.getElementById('map').style.flex = '';
  document.getElementById('placeholder-3d').style.display = 'none';

  const canvas = getCanvas();
  canvas.style.display = 'block';

  if (!renderer) initRenderer();

  disposeScene();
  pickableMeshes.length = 0;
  meshMeta.clear();
  sceneObjects.length = 0;
  setLabel('');
  document.getElementById('scene-legend').style.display = 'none';

  document.getElementById('panel-3d-title').textContent = tileId;

  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0e1117);
  scene.fog = new THREE.FogExp2(0x0e1117, 0.0003);

  // Fit canvas to panel before loading
  onWindowResize();

  // 1. Heightmap data is embedded in the HTML — no fetch needed.
  setLoading('Loading terrain...');
  const hmData = (window.tileHeightmaps || {{}})[tileId];
  const minH = hmData ? hmData.min_height_in : 0;
  const maxH = hmData ? hmData.max_height_in : 600;
  const minFt = minH / 12;
  const maxFt = maxH / 12;
  const heightRange = Math.max(maxFt - minFt, 1);

  // 2. Build terrain mesh with per-vertex displacement so normals reflect
  //    actual slope and lighting responds to terrain shape.
  //
  //    PlaneGeometry vertex layout (after rotateX(-PI/2)):
  //      index = row * 500 + col, row 0 = north (world Z = -250),
  //      row 499 = south (world Z = +249). col 0 = west, col 499 = east.
  //    PNG layout: row 0 = north, col 0 = west — matches exactly.
  let terrain = null;
  if (hmData) {{
    // Decode PNG via a canvas so we can read pixel values.
    const img = await new Promise((res, rej) => {{
      const i = new Image();
      i.onload = () => res(i);
      i.onerror = rej;
      i.src = hmData.url;
    }});
    const cvs = document.createElement('canvas');
    cvs.width = 500; cvs.height = 500;
    const ctx = cvs.getContext('2d');
    ctx.drawImage(img, 0, 0, 500, 500);
    const pixels = ctx.getImageData(0, 0, 500, 500).data;  // RGBA, row 0 = top of image = north

    const geo = new THREE.PlaneGeometry(500, 500, 499, 499);
    geo.rotateX(-Math.PI / 2);
    const pos = geo.attributes.position;
    for (let i = 0; i < pos.count; i++) {{
      const col = i % 500;
      const row = Math.floor(i / 500);  // row 0 = north = PNG row 0
      const t = pixels[(row * 500 + col) * 4] / 255;  // red channel = grey value
      pos.setY(i, minFt + t * heightRange);
    }}
    pos.needsUpdate = true;
    geo.computeVertexNormals();  // normals now follow actual slope → lighting works

    // Ortho texture — load from embedded data URI if available.
    const orthoUrl = (window.tileOrthoTextures || {{}})[tileId];
    let orthoTex = null;
    if (orthoUrl) {{
      const texLoader = new THREE.TextureLoader();
      orthoTex = await new Promise((res, rej) =>
        texLoader.load(orthoUrl, res, undefined, rej)
      ).catch(() => null);
    }}

    // If we have a photo texture use BasicMaterial so lighting doesn't
    // tint/wash out the aerial photograph colors.  Without a texture keep
    // StandardMaterial so the directional light reveals terrain contours.
    const mat = orthoTex
      ? new THREE.MeshBasicMaterial({{ map: orthoTex }})
      : new THREE.MeshStandardMaterial({{ color: 0xffffff, roughness: 0.85, metalness: 0.0 }});
    terrain = new THREE.Mesh(geo, mat);
    scene.add(terrain);
    sceneObjects.push({{ label: 'Terrain', color: '#ffffff', obj: terrain }});
  }} else {{
    const geo = new THREE.PlaneGeometry(500, 500, 1, 1);
    geo.rotateX(-Math.PI / 2);
    terrain = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({{ color: 0x3a5a2a, roughness: 1 }}));
    scene.add(terrain);
    sceneObjects.push({{ label: 'Terrain', color: '#3a5a2a', obj: terrain }});
  }}

  const ambient = new THREE.AmbientLight(0x334466, 0.5);
  scene.add(ambient);
  const sun = new THREE.DirectionalLight(0xfff8e8, 4.5);
  sun.position.set(-220, heightRange * 1.5 + 80, 180);
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0x6080b0, 0.4);
  fill.position.set(220, heightRange * 0.5, -180);
  scene.add(fill);

  // Position camera to look at tile from the SE at an oblique angle.
  const midFt = (minFt + maxFt) / 2;
  const camDist = Math.max(heightRange * 3, 300);
  controls.target.set(0, midFt, 0);
  camera.position.set(camDist * 0.6, midFt + camDist * 0.8, camDist * 0.9);
  camera.lookAt(0, midFt, 0);
  controls.update();

  // 3. Fresnel zone OBJ — exact geometry already generated by export_zone_obj().
  //    OBJ coordinate system: X=local easting, Y=local northing, Z=elevation (ft).
  //    applyObjTransform maps this to Three.js world (X=east, Y=elev up, Z=-north+250).
  setLoading('Loading Fresnel zone...');
  try {{
    const zoneObj = await loadOBJ(`obj-zone/${{tileId}}_zone.obj`);
    applyObjTransform(zoneObj);
    zoneObj.traverse(child => {{
      if (!child.isMesh) return;
      child.material = new THREE.MeshStandardMaterial({{
        color: 0xcc44ff,
        transparent: true, opacity: 0.5,
        depthWrite: false, side: THREE.DoubleSide,
        roughness: 0.4,
      }});
    }});
    scene.add(zoneObj);
    sceneObjects.push({{ label: 'Fresnel Zone', color: '#cc44ff', obj: zoneObj }});
  }} catch(e) {{ /* zone OBJ not available for this tile */ }}

  // 4. Load obstruction OBJs
  const tileFeature = (window.tilesData || {{}}).features?.find(f => f.properties.id === tileId);
  const obsList = tileFeature?.properties?.obstructions || [];
  let colorIdx = 0;

  for (const obs of obsList) {{
    const shortId = obs.id.slice(0, 8);
    setLoading(`Loading obstruction ${{shortId}}...`);
    try {{
      const obsObj = await loadOBJ(`obj/${{tileId}}_${{obs.id}}.obj`);
      applyObjTransform(obsObj);
      const color = OBS_COLORS[colorIdx++ % OBS_COLORS.length];
      obsObj.traverse(child => {{
        if (!child.isMesh) return;
        child.material = new THREE.MeshStandardMaterial({{
          color,
          roughness: 0.65,
          metalness: 0.05,
        }});
        pickableMeshes.push(child);
        meshMeta.set(child, obs);
      }});
      scene.add(obsObj);
      const hexColor = '#' + color.toString(16).padStart(6, '0');
      const typeLabel = obs.type.replace(/_/g, ' ');
      sceneObjects.push({{ label: `${{typeLabel}} · ${{obs.id.slice(0, 8)}}`, color: hexColor, obj: obsObj }});
    }} catch(e) {{ /* OBJ not available */ }}
  }}

  setLoading('');
  buildLegend();
  onWindowResize();
  setTimeout(onWindowResize, 100); // re-check after layout settles
}}

function buildLegend() {{
  const legend = document.getElementById('scene-legend');
  const list   = document.getElementById('scene-legend-list');
  list.innerHTML = '';
  if (sceneObjects.length === 0) {{ legend.style.display = 'none'; return; }}
  legend.style.display = '';
  sceneObjects.forEach((item, idx) => {{
    const row = document.createElement('div');
    row.className = 'legend-item';

    const cb = document.createElement('input');
    cb.type = 'checkbox'; cb.className = 'legend-toggle';
    cb.checked = true; cb.id = `lcb-${{idx}}`;
    cb.addEventListener('change', () => {{ item.obj.visible = cb.checked; }});

    const swatch = document.createElement('span');
    swatch.className = 'legend-swatch';
    swatch.style.background = item.color;

    const lbl = document.createElement('label');
    lbl.className = 'legend-label';
    lbl.htmlFor = `lcb-${{idx}}`;
    lbl.textContent = item.label;

    row.appendChild(cb);
    row.appendChild(swatch);
    row.appendChild(lbl);
    list.appendChild(row);
  }});
}}

function onCanvasPointerDown(event) {{
  // Simple single-tap pick (ignore drag start)
  const startX = event.clientX;
  const startY = event.clientY;
  const cvs = getCanvas();
  const onUp = (e) => {{
    cvs.removeEventListener('pointerup', onUp);
    if (Math.abs(e.clientX - startX) > 5 || Math.abs(e.clientY - startY) > 5) return;
    const rect = cvs.getBoundingClientRect();
    pointer.x =  ((e.clientX - rect.left)  / rect.width)  * 2 - 1;
    pointer.y = -((e.clientY - rect.top)   / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const hits = raycaster.intersectObjects(pickableMeshes);
    if (hits.length > 0) {{
      const meta = meshMeta.get(hits[0].object);
      if (meta) {{
        const typeLabel = meta.type.replace(/_/g, ' ');
        setLabel(`${{typeLabel}} — ${{meta.id}}`);
      }}
    }} else {{
      setLabel('');
    }}
  }};
  cvs.addEventListener('pointerup', onUp);
}}

// Expose to Leaflet non-module script
window.show3DTile = (tileId) => loadTile(tileId).catch(console.error);
</script>
</body>
</html>"""

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(html)
    print(f"  Map saved to: {output_path}")


def export_zone_obj(zone, tile_id: str, out_dir: Path) -> None:
    """Write a Minecraft-style Fresnel zone volume OBJ for one tile.

    Coordinate system matches tile_to_obj.py exactly:
        X = easting  (local usft, origin at tile SW corner)
        Y = northing (local usft, origin at tile SW corner)
        Z = elevation (usft)

    Each zone cell is rendered as a solid slab from its bottom height to its
    top height.  Flat top and bottom faces are emitted for every cell.
    Vertical step walls fill gaps between adjacent cells (same rule as the
    terrain OBJ: top-surface wall where zt > neighbour_zt, bottom-surface wall
    where zb < neighbour_zb).  Cells at the boundary of the zone (or tile)
    get a full closure wall from zb → zt on that side.
    """
    sw = _tile_sw_corner_nys(tile_id)
    if sw is None:
        return
    tile_e, tile_n = sw

    S = TILE_SIDE_USFT

    # Build dense tile-local arrays for zone cells that overlap this tile.
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
        return

    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"{tile_id}_zone.obj"

    vi = 1
    with open(out_path, "w") as f:
        f.write(f"# Fresnel zone volume mesh — tile {tile_id}\n")
        f.write("# 1 unit = 1 US survey foot\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation\n\n")
        f.write(f"o zone_{tile_id.replace('-', '_')}\n\n")

        for xi in range(S):
            for yi in range(S):
                if not in_zone[xi, yi]:
                    continue

                zt = float(top_z[xi, yi])
                zb = float(bot_z[xi, yi])
                x0, y0 = float(xi), float(yi)
                x1, y1 = x0 + 1.0, y0 + 1.0

                # Top face (CCW, normal +Z)
                f.write(f"v {x0} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y1} {zt:.3f}\n")
                f.write(f"v {x0} {y1} {zt:.3f}\n")
                o = vi
                f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                vi += 4

                # Bottom face (reversed winding, normal -Z)
                f.write(f"v {x0} {y1} {zb:.3f}\n")
                f.write(f"v {x1} {y1} {zb:.3f}\n")
                f.write(f"v {x1} {y0} {zb:.3f}\n")
                f.write(f"v {x0} {y0} {zb:.3f}\n")
                o = vi
                f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                vi += 4

                # Side walls — same edge convention as tile_to_obj.py.
                # ax,ay → bx,by traces the shared edge (CCW outward winding).
                for dxi, dyi, ax, ay, bx, by in (
                    ( 0, -1, x0, y0, x1, y0),  # south (-Y)
                    ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                    (+1,  0, x1, y0, x1, y1),  # east  (+X)
                    (-1,  0, x0, y1, x0, y0),  # west  (-X)
                ):
                    nxi, nyi = xi + dxi, yi + dyi
                    if 0 <= nxi < S and 0 <= nyi < S and in_zone[nxi, nyi]:
                        nzt = float(top_z[nxi, nyi])
                        nzb = float(bot_z[nxi, nyi])
                        # Top-surface step: fill gap where this cell is higher.
                        if zt > nzt:
                            f.write(f"v {ax} {ay} {nzt:.3f}\n")
                            f.write(f"v {bx} {by} {nzt:.3f}\n")
                            f.write(f"v {bx} {by} {zt:.3f}\n")
                            f.write(f"v {ax} {ay} {zt:.3f}\n")
                            o = vi
                            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                            vi += 4
                        # Bottom-surface step: fill gap where this cell is lower.
                        if zb < nzb:
                            f.write(f"v {ax} {ay} {zb:.3f}\n")
                            f.write(f"v {bx} {by} {zb:.3f}\n")
                            f.write(f"v {bx} {by} {nzb:.3f}\n")
                            f.write(f"v {ax} {ay} {nzb:.3f}\n")
                            o = vi
                            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                            vi += 4
                    else:
                        # No zone neighbour on this side — close with a full
                        # vertical wall from bottom to top.
                        f.write(f"v {ax} {ay} {zb:.3f}\n")
                        f.write(f"v {bx} {by} {zb:.3f}\n")
                        f.write(f"v {bx} {by} {zt:.3f}\n")
                        f.write(f"v {ax} {ay} {zt:.3f}\n")
                        o = vi
                        f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                        vi += 4

    print(f"  Zone OBJ saved: {out_path}  ({vi - 1:,} verts)")


def run(tile_dir="data/preprocessed", zone_obj_dir=None, obs_cache="data/obstructions"):
    tile_dir = Path(tile_dir)

    print("=== Step 2.1: Compute Fresnel zone ===")
    print(f"  GPS A : {GPS_A}")
    print(f"  GPS B : {GPS_B}")
    print(f"  freq  : {FREQUENCY_HZ / 1e9:.1f} GHz   alpha={ALPHA}")

    nys_a, nys_b = translate_to_nys_plane([GPS_A, GPS_B])
    print(f"  NYS A : ({nys_a[0]:.1f}, {nys_a[1]:.1f}, {nys_a[2]:.1f})")
    print(f"  NYS B : ({nys_b[0]:.1f}, {nys_b[1]:.1f}, {nys_b[2]:.1f})")

    zone = compute_fresnel_zone(nys_a, nys_b, FREQUENCY_HZ, ALPHA)
    h = zone.widths.shape[0]
    max_w = int(zone.widths.max())
    print(f"  Output: H={h} rows, max_width={max_w} cols")
    print(f"  x_base_offset={zone.x_base_offset}  y_base_offset={zone.y_base_offset}")

    print()
    print("=== Step 2.2: Identify tiles ===")
    print(f"  tile_dir: {tile_dir}")

    # Compute all tiles that geometrically intersect the zone, regardless of
    # what is already on disk.
    all_needed = identify_tiles(zone, require_exists=False)
    print(f"  {len(all_needed)} tile(s) intersect the zone:")

    # If S3 is configured, fetch any tiles not already in the local cache.
    if "LOS_S3_BUCKET" in os.environ and "LOS_S3_PREFIX" in os.environ:
        from lib.tiles.fetch import s3_fetcher_from_env
        fetcher = s3_fetcher_from_env(tile_dir)
        missing = [t for t in all_needed if not fetcher.is_cached(t)]
        if missing:
            print(f"  Fetching {len(missing)} tile(s) from S3 ...")
            fetcher.ensure_tiles(missing)
        else:
            print("  All tiles already cached — skipping S3 fetch")
    else:
        print("  LOS_S3_BUCKET / LOS_S3_PREFIX not set — using local tiles only")

    # Identify which tiles are actually present on disk (after any fetching).
    tiles = identify_tiles(zone, tile_dir)
    if len(tiles) < len(all_needed):
        missing_local = set(all_needed) - set(tiles)
        print(f"  Warning: {len(missing_local)} tile(s) not available locally: {sorted(missing_local)}")
    print(f"  {len(tiles)} tile(s) ready to load")

    print()
    print("=== Step 2.2b: Fetch obstructions ===")
    obs_dir = Path(obs_cache)
    obs_dir.mkdir(parents=True, exist_ok=True)

    from lib.obstructions.fetch import obs_fetcher_from_env
    obs_fetcher = obs_fetcher_from_env(obs_dir)
    if obs_fetcher is not None:
        print(f"  Fetching obstructions for {len(all_needed)} tile(s) from S3 ...")
        fetched_ids = obs_fetcher.ensure_for_tiles(all_needed)
        print(f"  {len(fetched_ids)} obstruction(s) available in cache")
    else:
        print("  LOS_OBS_S3_BUCKET / LOS_OBS_S3_PREFIX not set — using local obstructions only")

    print()
    print("=== Step 2.3: Load rasterized tiles ===")
    terrain = load_terrain_grid(zone, tiles, tile_dir, obstruction_types="*", obstruction_dir=obs_dir)
    valid_mask = (np.arange(terrain.heights.shape[1])[None, :] < zone.widths[:, None])
    n_cells = int(zone.widths.sum())
    n_nonzero = int((terrain.heights[valid_mask] > 0).sum())
    max_h = int(terrain.heights[valid_mask].max()) if n_cells > 0 else 0
    print(f"  Loaded {len(tiles)} tile(s) into TerrainGrid")
    print(f"  Total valid cells : {n_cells}")
    print(f"  Non-zero cells    : {n_nonzero}  ({100 * n_nonzero / max(n_cells, 1):.1f}%)")
    print(f"  Max terrain height: {max_h} in  ({max_h / 12:.1f} ft)")

    print()
    print("=== Step 2.4: Compute intersection ===")
    obstruction = compute_intersection(zone, terrain)
    valid_vals = obstruction.values[valid_mask]
    obstructed = (valid_vals > 0).sum()
    fully_blocked = (valid_vals >= 1.0).sum()
    mean_val = float(valid_vals.mean()) if n_cells > 0 else 0.0
    max_val = float(valid_vals.max()) if n_cells > 0 else 0.0
    print(f"  Obstructed cells (>0)  : {obstructed} / {n_cells}  ({100 * obstructed / max(n_cells, 1):.1f}%)")
    print(f"  Fully blocked  (>=1.0) : {fully_blocked} / {n_cells}  ({100 * fully_blocked / max(n_cells, 1):.1f}%)")
    print(f"  Mean obstruction level : {mean_val:.4f}")
    print(f"  Max  obstruction level : {max_val:.4f}")

    # Build per-tile obstruction metadata and raster images for the HTML map popup.
    tile_obs_info: dict[str, list[dict]] = {}
    obs_rasters: dict[str, str] = {}
    for obs_id in terrain.matched_obstruction_ids:
        json_path = obs_dir / f"{obs_id}.json"
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
        tif_path = obs_dir / meta.get("raster_file", f"{obs_id}.tif")
        b64 = _obs_raster_b64(tif_path)
        if b64 is not None:
            obs_rasters[obs_id] = b64

    print()
    print("=== Heightmap export ===")
    heightmap_dir = Path("data/heightmaps")
    tile_heightmaps: dict[str, dict] = {}
    n_exported = 0
    for tid in tiles:
        if export_heightmap(tid, tile_dir, heightmap_dir):
            n_exported += 1
    print(f"  Exported {n_exported} heightmap(s) to {heightmap_dir}/")

    # Load each PNG back as a data URI so it can be embedded directly in the HTML.
    # This lets tile_map.html work when opened via file:// without a local server.
    for tid in tiles:
        png_path = heightmap_dir / f"{tid}.png"
        meta_path = heightmap_dir / f"{tid}.json"
        if not png_path.exists() or not meta_path.exists():
            continue
        try:
            meta = json.loads(meta_path.read_text())
            b64 = base64.b64encode(png_path.read_bytes()).decode("ascii")
            tile_heightmaps[tid] = {
                "url": "data:image/png;base64," + b64,
                "min_height_in": meta["min_height_in"],
                "max_height_in": meta["max_height_in"],
            }
        except (ValueError, KeyError):
            pass
    print(f"  Embedded {len(tile_heightmaps)} heightmap(s) in HTML")

    print()
    print("=== Ortho textures ===")
    ortho_dir = Path("data/orthos")
    ortho_dir.mkdir(parents=True, exist_ok=True)
    from lib.ortho.fetch import ortho_fetcher_from_env
    ortho_fetcher = ortho_fetcher_from_env(ortho_dir)
    if ortho_fetcher is not None:
        print(f"  Fetching ortho JP2s for {len(tiles)} tile(s) from S3 ...")
        available = ortho_fetcher.ensure_for_tile_ids(tiles)
        print(f"  {len(available)} ortho file(s) available")
    else:
        print("  LOS_ORTHO_S3_BUCKET / LOS_ORTHO_S3_PREFIX not set — using local orthos only")
    tile_ortho_textures = _build_ortho_textures(tiles, ortho_dir)
    print(f"  Embedded {len(tile_ortho_textures)} ortho texture(s) in HTML")

    print()
    print("=== Visualization ===")
    generate_tile_map(
        tiles, nys_a, nys_b, Path("data/tile_map.html"),
        obstruction=obstruction, tile_obs_info=tile_obs_info, obs_rasters=obs_rasters,
        tile_heightmaps=tile_heightmaps, tile_ortho_textures=tile_ortho_textures,
    )

    if zone_obj_dir is not None:
        print()
        print("=== Zone OBJ export ===")
        obj_dir = Path(zone_obj_dir)
        for tile_id in tiles:
            export_zone_obj(zone, tile_id, obj_dir)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Run Part 2 obstruction-detection steps.")
    parser.add_argument(
        "tile_dir",
        nargs="?",
        default="data/preprocessed",
        help="Directory containing preprocessed tile pairs (default: data/preprocessed)",
    )
    parser.add_argument(
        "--zone-obj-dir",
        default=None,
        metavar="DIR",
        help="If set, export per-tile Fresnel zone OBJ files to this directory",
    )
    parser.add_argument(
        "--obs-cache",
        default="data/obstructions",
        metavar="DIR",
        help="Local cache directory for obstruction tif+json pairs (default: data/obstructions)",
    )
    args = parser.parse_args()
    run(args.tile_dir, zone_obj_dir=args.zone_obj_dir, obs_cache=args.obs_cache)
