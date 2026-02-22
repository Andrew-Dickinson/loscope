"""
Generate a self-contained HTML viewer for the preprocessed LiDAR tiles.
Tiles are placed at their correct relative positions using NYS coordinates.

Usage:
    python tools/visualize.py [preprocessed_dir] [output_html]

Defaults:
    preprocessed_dir = data/preprocessed
    output_html      = data/viewer.html
"""
import base64
import io
import json
import sys
from pathlib import Path

import numpy as np
import tifffile
from PIL import Image

PREPROCESSED = Path("data/preprocessed")
OUTPUT = Path("data/viewer.html")
TILE_SIZE = 500  # usft per tile side


def tile_to_png_b64(tif_path, scale_max):
    arr = tifffile.imread(str(tif_path)).astype(np.float32)
    scaled = np.clip(arr / scale_max * 255, 0, 255).astype(np.uint8)
    # arr axes: [easting, northing] → transpose + flip so north is at top
    img_arr = scaled.T[::-1, :]
    img = Image.fromarray(img_arr, mode="L")
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


def load_tiles(preprocessed_dir):
    tiles = []
    for jf in sorted(preprocessed_dir.glob("*.json")):
        tiles.append(json.loads(jf.read_text()))
    return tiles


def place_on_grid(tiles):
    """Convert NYS offsets to (col, row) grid indices. Row 0 = north, col 0 = west."""
    xs = [m["x_offset"] for m in tiles]
    ys = [m["y_offset"] for m in tiles]
    min_x, max_y = min(xs), max(ys)

    grid = {}
    for m in tiles:
        col = (m["x_offset"] - min_x) // TILE_SIZE
        row = (max_y - m["y_offset"]) // TILE_SIZE
        grid[(col, row)] = m

    n_cols = (max(xs) - min_x) // TILE_SIZE + 1
    n_rows = (max_y - min(ys)) // TILE_SIZE + 1
    return grid, n_cols, n_rows


def build_html(preprocessed_dir, tiles):
    global_max = max(
        tifffile.imread(str(preprocessed_dir / m["raster_file"])).max()
        for m in tiles
    )

    grid, n_cols, n_rows = place_on_grid(tiles)

    rows_html = []
    for row in range(n_rows):
        cells = []
        for col in range(n_cols):
            m = grid.get((col, row))
            if m is None:
                cells.append('<td class="gap"></td>')
                continue

            tif = preprocessed_dir / m["raster_file"]
            arr = tifffile.imread(str(tif))
            max_val = int(arr.max())
            b64 = tile_to_png_b64(tif, global_max)
            tid = m["tile_id"]
            tooltip = (
                f"{tid}  |  X {m['x_offset']}  Y {m['y_offset']}"
                f"  |  max {max_val / 12:.0f} ft"
            )
            cells.append(
                f'<td class="cell">'
                f'<img src="data:image/png;base64,{b64}" title="{tooltip}">'
                f'<div class="label">{tid}</div>'
                f"</td>"
            )
        rows_html.append("<tr>" + "".join(cells) + "</tr>")

    grid_html = "\n".join(rows_html)
    n_tiles = len(tiles)

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>LiDAR Tile Viewer</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: #111;
    color: #ccc;
    font-family: monospace;
    padding: 24px;
  }}
  h1 {{ color: #7ecfff; font-size: 18px; margin-bottom: 6px; }}
  p.info {{ color: #666; font-size: 12px; margin-bottom: 20px; }}
  .compass {{
    display: inline-grid;
    grid-template-columns: 28px auto 28px;
    grid-template-rows: 20px auto 20px;
    align-items: center;
  }}
  .compass-label {{
    text-align: center;
    font-size: 11px;
    color: #444;
    font-weight: bold;
  }}
  table {{ border-collapse: collapse; }}
  td {{
    width: 100px;
    height: 100px;
    padding: 2px;
  }}
  td.gap {{
    background: #151515;
    border: 1px dashed #1e1e1e;
  }}
  td.cell {{
    text-align: center;
    vertical-align: top;
    padding: 2px;
  }}
  td.cell img {{
    display: block;
    width: 96px;
    height: 96px;
    image-rendering: pixelated;
    border: 1px solid #2a2a2a;
    cursor: crosshair;
  }}
  td.cell img:hover {{ border-color: #7ecfff; }}
  .label {{ font-size: 9px; color: #7ecfff; margin-top: 2px; }}
</style>
</head>
<body>
<h1>LiDAR Preprocessed Tiles</h1>
<p class="info">
  {n_tiles} tiles &middot; {n_cols}&times;{n_rows} grid &middot; 500&times;500 usft each &middot;
  brightness scaled to global max &middot; dashed cells = no data
</p>
<div class="compass">
  <div></div><div class="compass-label">N</div><div></div>
  <div class="compass-label">W</div>
  <table>
{grid_html}
  </table>
  <div class="compass-label">E</div>
  <div></div><div class="compass-label">S</div><div></div>
</div>
</body>
</html>"""


def main():
    preprocessed_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else PREPROCESSED
    output = Path(sys.argv[2]) if len(sys.argv) > 2 else OUTPUT

    tiles = load_tiles(preprocessed_dir)
    if not tiles:
        print(f"No tiles found in {preprocessed_dir}")
        sys.exit(1)

    print(f"Loaded {len(tiles)} tiles, building grid...")
    html = build_html(preprocessed_dir, tiles)
    output.write_text(html)
    print(f"Wrote {output}  (open with: open {output})")


if __name__ == "__main__":
    main()
