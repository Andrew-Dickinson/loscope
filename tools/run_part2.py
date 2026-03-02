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

import numpy as np
from PIL import Image
from pyproj import Transformer

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone, translate_to_nys_plane
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset
from los_analyzer.tiles.identify import identify_tiles
from los_analyzer.tiles.intersect import compute_intersection
from los_analyzer.tiles.load import load_terrain_grid

GPS_A = (40.861448, -73.907696, 76.0)
GPS_B = ( 40.830477, -73.941012, 80.0)
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


def generate_tile_map(
    tile_ids: list[str],
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    output_path: Path,
    frequency_hz: float = FREQUENCY_HZ,
    alpha: float = ALPHA,
    obstruction=None,
) -> None:
    """Write an interactive Leaflet HTML map of selected tiles to output_path."""
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
            "properties": {"id": tile_id, "hasObstruction": tile_id in obstructed_tile_ids},
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

    # Fallback center (midpoint of LOS line in lat/lon) if no tiles are present.
    cx = (a_ll[1] + b_ll[1]) / 2
    cy = (a_ll[0] + b_ll[0]) / 2

    html = f"""\
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8"/>
<title>LOS Tile Map</title>
<link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"/>
<script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
<style>
  html, body, #map {{ height: 100%; margin: 0; padding: 0; }}
</style>
</head>
<body>
<div id="map"></div>
<script>
var map = L.map('map', {{maxZoom: 22}});
L.tileLayer('https://{{s}}.tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{
  attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
  maxNativeZoom: 19,
  maxZoom: 22
}}).addTo(map);

var tilesData = {tiles_js};
var losData = {los_js};
var ellipseData = {ellipse_js};
var tileOverlays = {tile_overlays_js};

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
    var label = f.properties.hasObstruction
      ? '<code>' + f.properties.id + '</code> <span style="color:#e63030;font-weight:bold">obstructed</span>'
      : '<code>' + f.properties.id + '</code>';
    l.bindPopup(label);
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
</body>
</html>"""

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(html)
    print(f"  Map saved to: {output_path}")


def run(tile_dir="data/preprocessed"):
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
        from los_analyzer.tiles.fetch import s3_fetcher_from_env
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
    print("=== Step 2.3: Load rasterized tiles ===")
    terrain = load_terrain_grid(zone, tiles, tile_dir, obstruction_types="*")
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

    print()
    print("=== Visualization ===")
    generate_tile_map(tiles, nys_a, nys_b, Path("data/tile_map.html"), obstruction=obstruction)


if __name__ == "__main__":
    td = sys.argv[1] if len(sys.argv) > 1 else "data/preprocessed"
    run(td)
