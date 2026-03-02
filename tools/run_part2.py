"""
Run Part 2 obstruction-detection steps 2.1 and 2.2.

Usage:
    python tools/run_part2.py [tile_dir]

Default tile_dir: data/preprocessed
Writes an interactive HTML tile map to data/tile_map.html.
"""
import json
import math
import sys
from pathlib import Path

from pyproj import Transformer

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone, translate_to_nys_plane
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset
from los_analyzer.tiles.identify import identify_tiles

GPS_A = (40.81399261450678, -73.9576824966002, 100.0)
GPS_B = (40.81669146433694, -73.93829606722406, 101.0)
FREQUENCY_HZ = 5_000_000_000
ALPHA = 0.8

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


def generate_tile_map(
    tile_ids: list[str],
    nys_a: tuple[float, float, float],
    nys_b: tuple[float, float, float],
    output_path: Path,
    frequency_hz: float = FREQUENCY_HZ,
    alpha: float = ALPHA,
) -> None:
    """Write an interactive Leaflet HTML map of selected tiles to output_path."""
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
            "properties": {"id": tile_id},
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
var map = L.map('map');
L.tileLayer('https://{{s}}.tile.openstreetmap.org/{{z}}/{{x}}/{{y}}.png', {{
  attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'
}}).addTo(map);

var tilesData = {tiles_js};
var losData = {los_js};
var ellipseData = {ellipse_js};

if (ellipseData) {{
  L.geoJSON(ellipseData, {{
    style: {{ color: '#ff8800', weight: 2, dashArray: '8 5', fillOpacity: 0.08, fillColor: '#ff8800' }}
  }}).addTo(map);
}}

var tileLayer = L.geoJSON(tilesData, {{
  style: {{ color: '#3388ff', weight: 1, fillOpacity: 0.3, fillColor: '#3388ff' }},
  onEachFeature: function(f, l) {{
    l.bindPopup('<code>' + f.properties.id + '</code>');
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

var bounds = tileLayer.getBounds();
if (bounds.isValid()) {{
  map.fitBounds(bounds.pad(0.3));
}} else {{
  map.setView([{cx:.6f}, {cy:.6f}], 13);
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
    tiles = identify_tiles(zone, tile_dir, require_exists=False)
    print(f"  {len(tiles)} tile(s) found:")
    for t in tiles:
        print(f"    {t}")

    print()
    print("=== Visualization ===")
    generate_tile_map(tiles, nys_a, nys_b, Path("data/tile_map.html"))


if __name__ == "__main__":
    td = sys.argv[1] if len(sys.argv) > 1 else "data/preprocessed"
    run(td)
