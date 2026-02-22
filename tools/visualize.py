"""
Generate a self-contained HTML viewer for the preprocessed LiDAR tiles.

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

GRID_N = 5


def tile_to_png_b64(tif_path, scale_max):
    arr = tifffile.imread(str(tif_path)).astype(np.float32)
    scaled = np.clip(arr / scale_max * 255, 0, 255).astype(np.uint8)
    # arr is [easting, northing]; transpose so rows=northing, cols=easting,
    # then flip rows so north is at the top of the image.
    img_arr = scaled.T[::-1, :]
    img = Image.fromarray(img_arr, mode="L")
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


def load_metadata(preprocessed_dir):
    meta = {}
    for jf in sorted(preprocessed_dir.glob("*.json")):
        m = json.loads(jf.read_text())
        tid = m["tile_id"]
        parts = tid.rsplit("_", 1)
        if len(parts) == 2 and len(parts[1]) == 2:
            xi, yi = int(parts[1][0]), int(parts[1][1])
            meta[(xi, yi)] = m
    return meta


def build_html(preprocessed_dir, metadata):
    # Compute global max across all tiles for uniform brightness scaling
    global_max = max(
        tifffile.imread(str(preprocessed_dir / m["raster_file"])).max()
        for m in metadata.values()
    )

    rows = []
    for yi in range(GRID_N - 1, -1, -1):   # yi=4 (north) → yi=0 (south)
        cells = []
        for xi in range(GRID_N):            # xi=0 (west) → xi=4 (east)
            m = metadata.get((xi, yi))
            if m is None:
                cells.append("<td></td>")
                continue

            tif = preprocessed_dir / m["raster_file"]
            arr = tifffile.imread(str(tif))
            max_val = int(arr.max())
            max_ft = max_val / 12

            b64 = tile_to_png_b64(tif, global_max)
            tid = m["tile_id"]
            tooltip = (
                f"{tid}  |  X {m['x_offset']}  Y {m['y_offset']}"
                f"  |  max {max_ft:.0f} ft ({max_val} in)"
            )
            cells.append(
                f'<td class="cell">'
                f'<img src="data:image/png;base64,{b64}" title="{tooltip}">'
                f'<div class="label">{tid}</div>'
                f'<div class="sub">{max_ft:.0f} ft</div>'
                f"</td>"
            )
        rows.append("<tr>" + "".join(cells) + "</tr>")

    grid_html = "\n".join(rows)

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
    grid-template-columns: 36px auto 36px;
    grid-template-rows: 24px auto 24px;
    gap: 0;
    align-items: center;
  }}
  .compass-label {{
    text-align: center;
    font-size: 12px;
    color: #555;
    font-weight: bold;
  }}
  table {{ border-collapse: collapse; }}
  td.cell {{
    padding: 3px;
    text-align: center;
    vertical-align: top;
  }}
  td.cell img {{
    display: block;
    width: 180px;
    height: 180px;
    image-rendering: pixelated;
    border: 1px solid #222;
    cursor: crosshair;
  }}
  td.cell img:hover {{ border-color: #7ecfff; }}
  .label {{ font-size: 11px; color: #7ecfff; margin-top: 3px; }}
  .sub {{ font-size: 10px; color: #555; margin-top: 1px; }}
</style>
</head>
<body>
<h1>LiDAR Preprocessed Tiles</h1>
<p class="info">
  25 tiles &middot; 500&times;500 usft each &middot;
  brightness scaled to global max height &middot;
  hover for coordinates &amp; max height
</p>
<div class="compass">
  <div></div>
  <div class="compass-label">N</div>
  <div></div>
  <div class="compass-label">W</div>
  <table>
{grid_html}
  </table>
  <div class="compass-label">E</div>
  <div></div>
  <div class="compass-label">S</div>
  <div></div>
</div>
</body>
</html>"""


def main():
    preprocessed_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else PREPROCESSED
    output = Path(sys.argv[2]) if len(sys.argv) > 2 else OUTPUT

    tifs = list(preprocessed_dir.glob("*.tif"))
    if not tifs:
        print(f"No .tif files found in {preprocessed_dir}")
        sys.exit(1)

    print(f"Loading {len(tifs)} tiles from {preprocessed_dir}...")
    metadata = load_metadata(preprocessed_dir)
    html = build_html(preprocessed_dir, metadata)

    output.write_text(html)
    print(f"Wrote {output}")
    print(f"Open with: open {output}")


if __name__ == "__main__":
    main()
