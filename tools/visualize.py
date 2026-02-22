"""
Generate a self-contained HTML viewer for the preprocessed LiDAR tiles.
Tiles are placed at their correct relative positions using NYS coordinates.
Shows a 14×14 viewport (~196 tiles) with click-to-navigate controls.

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
TILE_SIZE = 500   # usft per tile side
VIEW_COLS = 20
VIEW_ROWS = 10


def tile_to_png_b64(tif_path, scale_max):
    arr = tifffile.imread(str(tif_path)).astype(np.float32)
    scaled = np.clip(arr / scale_max * 255, 0, 255).astype(np.uint8)
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

    tile_data = {}
    for (col, row), m in grid.items():
        tif = preprocessed_dir / m["raster_file"]
        arr = tifffile.imread(str(tif))
        max_val = int(arr.max())
        b64 = tile_to_png_b64(tif, global_max)
        tid = m["tile_id"]
        tooltip = (
            f"{tid}  |  X {m['x_offset']}  Y {m['y_offset']}"
            f"  |  max {max_val / 12:.0f} ft"
        )
        tile_data[f"{col},{row}"] = {"id": tid, "b64": b64, "tip": tooltip}

    n_tiles = len(tiles)
    tile_data_json = json.dumps(tile_data)

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
    user-select: none;
  }}
  h1 {{ color: #7ecfff; font-size: 18px; margin-bottom: 6px; }}
  p.info {{ color: #666; font-size: 12px; margin-bottom: 16px; }}
  #viewport-pos {{ color: #555; font-size: 11px; margin-bottom: 12px; }}
  .nav {{
    display: inline-grid;
    grid-template-columns: 32px 32px 32px;
    grid-template-rows: 28px 28px 28px;
    gap: 3px;
    margin-bottom: 16px;
  }}
  .nav button {{
    background: #1e1e1e;
    color: #7ecfff;
    border: 1px solid #333;
    border-radius: 3px;
    cursor: pointer;
    font-size: 15px;
    line-height: 1;
  }}
  .nav button:hover {{ background: #2a2a2a; border-color: #7ecfff; }}
  .nav .blank {{ border: none; background: none; cursor: default; }}
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
  {n_tiles} tiles &middot; {n_cols}&times;{n_rows} grid &middot;
  500&times;500 usft each &middot; showing {VIEW_COLS}&times;{VIEW_ROWS} at a time
</p>
<div class="nav">
  <button class="blank"></button>
  <button onclick="pan(0,-1)" title="North (↑)">↑</button>
  <button class="blank"></button>
  <button onclick="pan(-1,0)" title="West (←)">←</button>
  <button class="blank"></button>
  <button onclick="pan(1,0)" title="East (→)">→</button>
  <button class="blank"></button>
  <button onclick="pan(0,1)" title="South (↓)">↓</button>
  <button class="blank"></button>
</div>
<div id="viewport-pos"></div>
<div id="grid"></div>

<script>
const TILES = {tile_data_json};
const N_COLS = {n_cols};
const N_ROWS = {n_rows};
const VIEW_COLS = {VIEW_COLS};
const VIEW_ROWS = {VIEW_ROWS};

let ox = 0, oy = 0;

function pan(dc, dr) {{
  ox = Math.max(0, Math.min(N_COLS - VIEW_COLS, ox + dc));
  oy = Math.max(0, Math.min(N_ROWS - VIEW_ROWS, oy + dr));
  render();
}}

function render() {{
  const cols = Math.min(VIEW_COLS, N_COLS - ox);
  const rows = Math.min(VIEW_ROWS, N_ROWS - oy);

  const table = document.createElement('table');
  for (let r = oy; r < oy + rows; r++) {{
    const tr = document.createElement('tr');
    for (let c = ox; c < ox + cols; c++) {{
      const td = document.createElement('td');
      const tile = TILES[c + ',' + r];
      if (tile) {{
        td.className = 'cell';
        const img = document.createElement('img');
        img.src = 'data:image/png;base64,' + tile.b64;
        img.title = tile.tip;
        const lbl = document.createElement('div');
        lbl.className = 'label';
        lbl.textContent = tile.id;
        td.appendChild(img);
        td.appendChild(lbl);
      }} else {{
        td.className = 'gap';
      }}
      tr.appendChild(td);
    }}
    table.appendChild(tr);
  }}

  const el = document.getElementById('grid');
  el.innerHTML = '';
  el.appendChild(table);

  document.getElementById('viewport-pos').textContent =
    `cols ${{ox}}–${{ox + cols - 1}} / ${{N_COLS - 1}}  ·  rows ${{oy}}–${{oy + rows - 1}} / ${{N_ROWS - 1}}  (N is row 0)`;
}}

document.addEventListener('keydown', e => {{
  if (e.key === 'ArrowUp')    {{ pan(0, -1); e.preventDefault(); }}
  if (e.key === 'ArrowDown')  {{ pan(0,  1); e.preventDefault(); }}
  if (e.key === 'ArrowLeft')  {{ pan(-1, 0); e.preventDefault(); }}
  if (e.key === 'ArrowRight') {{ pan( 1, 0); e.preventDefault(); }}
}});

render();
</script>
</body>
</html>"""


def main():
    preprocessed_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else PREPROCESSED
    output = Path(sys.argv[2]) if len(sys.argv) > 2 else OUTPUT

    tiles = load_tiles(preprocessed_dir)
    if not tiles:
        print(f"No tiles found in {preprocessed_dir}")
        sys.exit(1)

    print(f"Loaded {len(tiles)} tiles, building viewer...")
    html = build_html(preprocessed_dir, tiles)
    output.write_text(html)
    print(f"Wrote {output}  (open with: open {output})")


if __name__ == "__main__":
    main()
