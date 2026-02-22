"""Visualize the Fresnel zone for a radio link as a self-contained HTML viewer.

Generates two images:
  - Plan view:      top-down footprint of the Fresnel zone, coloured by
                    vertical half-width (brighter = more clearance needed).
  - Cross-section:  vertical slice along the LOS showing the ellipse outline
                    and LOS altitude.

Usage:
    python tools/visualize_fresnel.py [output.html] [--options]

Options:
    --lat-a  LAT   Endpoint A latitude  (default 40.7128)
    --lon-a  LON   Endpoint A longitude (default -74.0060)
    --alt-a  ALT   Endpoint A altitude in metres (default 100.0)
    --lat-b  LAT   Endpoint B latitude  (default 40.7173)
    --lon-b  LON   Endpoint B longitude (default -74.0060)
    --alt-b  ALT   Endpoint B altitude in metres (default 105.0)
    --freq   GHZ   Frequency in GHz (default 2.4)
    --alpha  A     Fresnel zone scale factor (default 1.0)
    --output PATH  Output HTML file (default data/fresnel_viewer.html)
"""
from __future__ import annotations

import argparse
import base64
import io
import sys
from pathlib import Path

import numpy as np
import pyproj
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from los_analyzer.fresnel.fresnel_zone import (
    SPEED_OF_LIGHT_M_S,
    USFT_PER_METER,
    FresnelZone,
    compute_fresnel_zone,
)

# ── helpers ──────────────────────────────────────────────────────────────────

def _font(size: int = 11):
    try:
        return ImageFont.load_default(size=size)
    except TypeError:
        return ImageFont.load_default()


def _to_b64(img: Image.Image) -> str:
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


def _link_stats(
    point_a, point_b, freq_hz: float, alpha: float, fz: FresnelZone
) -> dict:
    latA, lonA, altA = point_a
    latB, lonB, altB = point_b

    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    ecef_crs = pyproj.CRS.from_epsg(4978)
    t_ecef = pyproj.Transformer.from_crs(gps_crs, ecef_crs, always_xy=False)
    A_ecef = np.array(t_ecef.transform(latA, lonA, altA))
    B_ecef = np.array(t_ecef.transform(latB, lonB, altB))
    L_m = float(np.linalg.norm(B_ecef - A_ecef))
    L_usft = L_m * USFT_PER_METER

    wavelength_m = SPEED_OF_LIGHT_M_S / freq_hz
    r_max_m = alpha * np.sqrt(wavelength_m * L_m / 4)
    r_max_usft = r_max_m * USFT_PER_METER

    W, H = fz.mask.shape
    return dict(
        L_m=L_m, L_usft=L_usft,
        r_max_m=r_max_m, r_max_usft=r_max_usft,
        wavelength_m=wavelength_m,
        W=W, H=H,
        mask_cells=int(fz.mask.sum()),
    )


def _nys_endpoints(point_a, point_b):
    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    t = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)
    xA, yA, zA = t.transform(*point_a)
    xB, yB, zB = t.transform(*point_b)
    return (xA, yA, zA), (xB, yB, zB)


# ── plan view ────────────────────────────────────────────────────────────────

def render_plan_view(
    fz: FresnelZone,
    xA: float, yA: float,
    xB: float, yB: float,
    max_px: int = 1000,
) -> Image.Image:
    """Top-down view of the Fresnel zone footprint.

    Colour encodes vertical half-width (brighter = deeper inside the ellipse).
    Yellow line = LOS.  Red dot = A, blue dot = B.
    """
    W, H = fz.mask.shape   # [easting, northing]

    # Vertical half-width for colouring
    z_mid = (fz.top + fz.bottom) * 0.5
    v = np.where(fz.mask, fz.top - z_mid, 0.0)
    v_max = float(v.max()) if v.max() > 0 else 1.0

    # RGBA array indexed [easting, northing]
    rgba = np.full((W, H, 4), (20, 22, 30, 255), dtype=np.uint8)
    m = fz.mask == 1
    v_n = v[m] / v_max  # 0 = edge, 1 = centre
    rgba[m, 0] = (v_n * 30).astype(np.uint8)
    rgba[m, 1] = (55 + v_n * 170).astype(np.uint8)
    rgba[m, 2] = (100 + v_n * 100).astype(np.uint8)

    # Transpose to PIL layout (rows=northing, cols=easting), then flip N↑
    pil_arr = rgba.transpose(1, 0, 2)   # shape (H, W, 4)
    pil_arr = pil_arr[::-1, :, :]       # row 0 = max northing = north

    img = Image.fromarray(pil_arr, mode="RGBA")

    # Resize so the longer dimension is ≤ max_px
    scale_x = scale_y = 1.0
    img_w, img_h = img.size   # (W, H) in PIL = (easting_px, northing_px)
    if max(img_w, img_h) > max_px:
        s = max_px / max(img_w, img_h)
        new_w = max(4, int(img_w * s))
        new_h = max(4, int(img_h * s))
        scale_x = new_w / W
        scale_y = new_h / H
        img = img.resize((new_w, new_h), Image.NEAREST)
        img_w, img_h = new_w, new_h
    else:
        scale_x = 1.0
        scale_y = 1.0

    def to_px(x: float, y: float) -> tuple[int, int]:
        """NYS (easting, northing) → image (col, row)."""
        col = int((x - fz.x_offset) * scale_x)
        row = img_h - 1 - int((y - fz.y_offset) * scale_y)
        return col, row

    draw = ImageDraw.Draw(img)
    font = _font(10)

    # LOS line
    draw.line([to_px(xA, yA), to_px(xB, yB)], fill=(255, 220, 0, 180), width=2)

    # Endpoint markers
    for (x, y), color, label in [
        ((xA, yA), (255, 80, 80), "A"),
        ((xB, yB), (80, 80, 255), "B"),
    ]:
        cx, cy = to_px(x, y)
        r = 5
        draw.ellipse([cx - r, cy - r, cx + r, cy + r],
                     fill=(*color, 255), outline=(255, 255, 255, 255), width=1)
        draw.text((cx + r + 2, cy - r), label, fill=(255, 255, 255, 220), font=font)

    return img


# ── cross-section ─────────────────────────────────────────────────────────────

def render_cross_section(
    zA: float, zB: float,
    L_m: float,
    freq_hz: float,
    alpha: float,
    width: int = 900,
    height: int = 340,
) -> Image.Image:
    """Vertical cross-section along the LOS showing the Fresnel ellipse."""
    N = 600
    t = np.linspace(0.0, 1.0, N)

    z_los = zA + t * (zB - zA)

    wavelength_m = SPEED_OF_LIGHT_M_S / freq_hz
    d1 = t * L_m
    d2 = (1.0 - t) * L_m
    safe_denom = np.where(d1 + d2 > 0, d1 + d2, 1.0)
    r_m = alpha * np.sqrt(wavelength_m * d1 * d2 / safe_denom)
    r_usft = r_m * USFT_PER_METER

    top = z_los + r_usft
    bot = z_los - r_usft

    z_center = (zA + zB) / 2
    r_max = r_usft.max()
    pad = max(r_max * 0.3, 2.0)
    y_min = min(bot.min(), zA, zB) - pad
    y_max = max(top.max(), zA, zB) + pad

    ML, MR, MT, MB = 70, 20, 20, 40
    pw = width - ML - MR
    ph = height - MT - MB

    def px(ti: float, zi: float) -> tuple[int, int]:
        return (
            ML + int(ti * pw),
            MT + ph - int((zi - y_min) / (y_max - y_min) * ph),
        )

    img = Image.new("RGBA", (width, height), (20, 22, 30, 255))
    draw = ImageDraw.Draw(img)
    font_sm = _font(10)
    font_md = _font(11)

    # Grid lines (horizontal altitude ticks)
    step_raw = (y_max - y_min) / 6
    magnitude = 10 ** int(np.floor(np.log10(step_raw)))
    step = round(step_raw / magnitude) * magnitude
    z_tick = np.ceil(y_min / step) * step
    while z_tick <= y_max:
        _, py_tick = px(0, z_tick)
        draw.line([(ML, py_tick), (ML + pw, py_tick)], fill=(40, 42, 50, 255), width=1)
        draw.text((4, py_tick - 6), f"{z_tick:.0f}", fill=(100, 100, 110, 255), font=font_sm)
        z_tick += step

    # Vertical grid lines (distance ticks)
    for frac in [0.25, 0.5, 0.75]:
        x_tick = ML + int(frac * pw)
        draw.line([(x_tick, MT), (x_tick, MT + ph)], fill=(40, 42, 50, 255), width=1)
        dist_m = frac * L_m
        draw.text((x_tick - 16, MT + ph + 4), f"{dist_m:.0f}m",
                  fill=(90, 90, 100, 255), font=font_sm)

    # Axis labels
    draw.text((ML, MT + ph + 22), "0m", fill=(90, 90, 100, 255), font=font_sm)
    draw.text((ML + pw - 24, MT + ph + 22), f"{L_m:.0f}m",
              fill=(90, 90, 100, 255), font=font_sm)
    draw.text((ML + pw // 2 - 40, height - 14),
              "Distance along LOS", fill=(90, 90, 100, 255), font=font_sm)
    draw.text((4, MT + ph // 2 - 20), "Alt\n(usft)", fill=(90, 90, 100, 255), font=font_sm)

    # Filled Fresnel band
    pts_top = [px(t[i], top[i]) for i in range(N)]
    pts_bot = [px(t[i], bot[i]) for i in range(N - 1, -1, -1)]
    draw.polygon(pts_top + pts_bot, fill=(0, 160, 90, 60))

    # Top / bottom outlines
    for i in range(N - 1):
        draw.line([pts_top[i], pts_top[i + 1]], fill=(0, 210, 120, 220), width=1)
    for i in range(N - 1):
        draw.line([pts_bot[N - 1 - i - 1], pts_bot[N - 1 - i]],
                  fill=(0, 210, 120, 220), width=1)

    # LOS line
    los_pts = [px(t[i], z_los[i]) for i in range(N)]
    for i in range(N - 1):
        draw.line([los_pts[i], los_pts[i + 1]], fill=(255, 220, 0, 230), width=2)

    # Endpoint altitude markers (dashed horizontal)
    for z_ep, color in [(zA, (255, 80, 80, 160)), (zB, (80, 80, 255, 160))]:
        _, py_ep = px(0, z_ep)
        for xx in range(ML, ML + pw, 8):
            draw.line([(xx, py_ep), (xx + 4, py_ep)], fill=color, width=1)

    # Endpoint dots
    for frac, z_ep, color, label in [
        (0.0, zA, (255, 80, 80), "A"),
        (1.0, zB, (80, 80, 255), "B"),
    ]:
        cx, cy = px(frac, z_ep)
        r = 5
        draw.ellipse([cx - r, cy - r, cx + r, cy + r],
                     fill=(*color, 255), outline=(255, 255, 255, 200), width=1)
        draw.text((cx + r + 2, cy - 7), label, fill=(255, 255, 255, 200), font=font_md)

    # Border
    draw.rectangle([ML, MT, ML + pw, MT + ph], outline=(50, 52, 60, 255), width=1)

    return img


# ── HTML builder ──────────────────────────────────────────────────────────────

def build_html(
    point_a, point_b,
    freq_hz: float,
    alpha: float,
    fz: FresnelZone,
    plan_img: Image.Image,
    xs_img: Image.Image,
    stats: dict,
) -> str:
    plan_b64 = _to_b64(plan_img)
    xs_b64 = _to_b64(xs_img)
    latA, lonA, altA = point_a
    latB, lonB, altB = point_b
    freq_ghz = freq_hz / 1e9
    plan_w, plan_h = plan_img.size
    xs_w, xs_h = xs_img.size

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Fresnel Zone Viewer</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    background: #111;
    color: #ccc;
    font-family: monospace;
    padding: 28px;
  }}
  h1 {{ color: #7ecfff; font-size: 18px; margin-bottom: 4px; }}
  h2 {{ color: #7ecfff; font-size: 13px; margin: 20px 0 8px; font-weight: normal; letter-spacing: .08em; text-transform: uppercase; }}
  .subtitle {{ color: #555; font-size: 12px; margin-bottom: 20px; }}
  .row {{ display: flex; gap: 28px; align-items: flex-start; flex-wrap: wrap; }}
  .col {{ display: flex; flex-direction: column; gap: 20px; }}
  .panel {{ display: flex; flex-direction: column; gap: 8px; }}
  .panel-title {{ color: #7ecfff; font-size: 11px; letter-spacing: .1em; text-transform: uppercase; }}
  img.plan {{
    display: block;
    image-rendering: pixelated;
    width: {plan_w}px;
    height: {plan_h}px;
    border: 1px solid #2a2a2a;
    border-radius: 3px;
  }}
  img.xs {{
    display: block;
    image-rendering: pixelated;
    width: {xs_w}px;
    height: {xs_h}px;
    border: 1px solid #2a2a2a;
    border-radius: 3px;
  }}
  table.stats {{
    border-collapse: collapse;
    font-size: 12px;
    margin-top: 4px;
  }}
  table.stats tr {{ border-bottom: 1px solid #1e1e1e; }}
  table.stats td {{ padding: 4px 12px 4px 0; }}
  table.stats td:first-child {{ color: #666; white-space: nowrap; }}
  .legend {{
    display: flex;
    gap: 16px;
    font-size: 11px;
    margin-top: 6px;
    color: #888;
  }}
  .swatch {{ display: inline-block; width: 14px; height: 10px; border-radius: 2px; vertical-align: middle; margin-right: 4px; }}
</style>
</head>
<body>
<h1>Fresnel Zone Viewer</h1>
<p class="subtitle">
  A ({latA:.5f}, {lonA:.5f}, {altA:.1f} m)
  &rarr; B ({latB:.5f}, {lonB:.5f}, {altB:.1f} m)
  &nbsp;&middot;&nbsp; {freq_ghz:.2f} GHz &nbsp;&middot;&nbsp; &alpha; = {alpha}
</p>

<div class="row">
  <!-- Left: plan view -->
  <div class="panel">
    <span class="panel-title">Plan view (top-down footprint)</span>
    <img class="plan" src="data:image/png;base64,{plan_b64}"
         title="Top-down Fresnel zone. Colour = vertical half-width (brighter = deeper inside ellipse)." />
    <div class="legend">
      <span><span class="swatch" style="background:#00d478"></span>Fresnel mask (bright = centre)</span>
      <span><span class="swatch" style="background:#ffdc00"></span>LOS</span>
      <span><span class="swatch" style="background:#ff5050"></span>A</span>
      <span><span class="swatch" style="background:#5050ff"></span>B</span>
    </div>
  </div>

  <!-- Right: cross-section + stats stacked -->
  <div class="col">
    <div class="panel">
      <span class="panel-title">Cross-section along LOS</span>
      <img class="xs" src="data:image/png;base64,{xs_b64}"
           title="Vertical cross-section along LOS. Green band = Fresnel zone. Yellow = LOS." />
      <div class="legend">
        <span><span class="swatch" style="background:#00d478"></span>Fresnel top / bottom</span>
        <span><span class="swatch" style="background:#ffdc00"></span>LOS altitude</span>
        <span><span class="swatch" style="background:#ff5050"></span>A altitude</span>
        <span><span class="swatch" style="background:#5050ff"></span>B altitude</span>
      </div>
    </div>

    <div class="panel">
      <span class="panel-title">Link parameters</span>
      <table class="stats">
        <tr><td>Frequency</td><td>{freq_ghz:.3f} GHz</td></tr>
        <tr><td>Wavelength</td><td>{stats['wavelength_m']*100:.2f} cm</td></tr>
        <tr><td>Alpha (&alpha;)</td><td>{alpha:.2f}</td></tr>
        <tr><td>Link length</td><td>{stats['L_m']:.1f} m &nbsp; ({stats['L_usft']:.0f} usft)</td></tr>
        <tr><td>Max Fresnel radius</td><td>{stats['r_max_m']:.3f} m &nbsp; ({stats['r_max_usft']:.2f} usft)</td></tr>
        <tr><td>Grid size (W &times; H)</td><td>{stats['W']} &times; {stats['H']} usft</td></tr>
        <tr><td>Mask cells</td><td>{stats['mask_cells']:,}</td></tr>
        <tr><td>x_offset</td><td>{fz.x_offset:,} usft</td></tr>
        <tr><td>y_offset</td><td>{fz.y_offset:,} usft</td></tr>
      </table>
    </div>
  </div>
</div>

</body>
</html>"""


# ── main ──────────────────────────────────────────────────────────────────────

def parse_args():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lat-a",  type=float, default=40.7128,  metavar="LAT")
    p.add_argument("--lon-a",  type=float, default=-74.0060, metavar="LON")
    p.add_argument("--alt-a",  type=float, default=100.0,    metavar="ALT_M")
    p.add_argument("--lat-b",  type=float, default=40.7173,  metavar="LAT")
    p.add_argument("--lon-b",  type=float, default=-74.0060, metavar="LON")
    p.add_argument("--alt-b",  type=float, default=105.0,    metavar="ALT_M")
    p.add_argument("--freq",   type=float, default=2.4,      metavar="GHZ",
                   help="Frequency in GHz (default 2.4)")
    p.add_argument("--alpha",  type=float, default=1.0,      metavar="A",
                   help="Fresnel zone scale factor (default 1.0)")
    p.add_argument("--output", type=Path,
                   default=Path("../data/fresnel_viewer.html"), metavar="PATH")
    return p.parse_args()


def main():
    args = parse_args()

    point_a = (args.lat_a, args.lon_a, args.alt_a)
    point_b = (args.lat_b, args.lon_b, args.alt_b)
    freq_hz = args.freq * 1e9

    print(f"Computing Fresnel zone …")
    fz = compute_fresnel_zone(point_a, point_b, freq_hz, alpha=args.alpha)
    print(f"  Grid: {fz.mask.shape[0]} × {fz.mask.shape[1]} usft  "
          f"({fz.mask.sum():,} masked cells)")

    (xA, yA, zA), (xB, yB, zB) = _nys_endpoints(point_a, point_b)
    stats = _link_stats(point_a, point_b, freq_hz, args.alpha, fz)

    print("Rendering plan view …")
    plan_img = render_plan_view(fz, xA, yA, xB, yB)

    print("Rendering cross-section …")
    xs_img = render_cross_section(zA, zB, stats["L_m"], freq_hz, args.alpha)

    print("Building HTML …")
    html = build_html(point_a, point_b, freq_hz, args.alpha,
                      fz, plan_img, xs_img, stats)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(html)
    print(f"Wrote {args.output}  →  open {args.output}")


if __name__ == "__main__":
    main()
