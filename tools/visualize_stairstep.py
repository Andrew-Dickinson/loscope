#!/usr/bin/env python3
"""
Visualize stairstep debug dumps from the LOS backend.

Usage:
    python visualize_stairstep.py <dump_dir> [--tile <tile_id>]

<dump_dir> is the per-analysis directory written when LOS_DEBUG_DUMP_DIR is set,
e.g. /tmp/los_debug/3f4a1b2c-.... It should contain:
    zone_full.json
    zone_inner.json
    terrain_full.json
    terrain_inner.json
    intersection_full.json
    intersection_inner.json

--tile <tile_id>  Zoom all plots to this tile, e.g. --tile 987200_10

Requires: numpy, matplotlib
"""

import argparse
import json
from pathlib import Path

import numpy as np
import matplotlib.pyplot as plt

# ── Tile ID parsing (mirrors tiles.rs) ────────────────────────────────────────

_SUBGRID_TILE_SIDE = 500   # usft
_EASTING_ROLLOVER_BOUND = 500
_EASTING_ROLLOVER_POINT = 1000


def _component_to_usft(c: int) -> int:
    base = c * 1000
    return base + 500 if c % 10 in (2, 7) else base


def parse_tile_sw_corner(tile_str: str) -> tuple[int, int]:
    """Return (sw_easting, sw_northing) in usft for a tile ID like '987200_10'."""
    las_str, sub_str = tile_str.split("_")

    northing_start = max(0, len(las_str) - 3)
    northing_base = int(las_str[northing_start:])
    easting_base = int(las_str[:northing_start]) if northing_start > 0 else _EASTING_ROLLOVER_POINT
    if easting_base < _EASTING_ROLLOVER_BOUND:
        easting_base += _EASTING_ROLLOVER_POINT

    las_sw_e = _component_to_usft(easting_base)
    las_sw_n = _component_to_usft(northing_base)

    sub_x = int(sub_str[0])
    sub_y = int(sub_str[1])

    return las_sw_e + sub_x * _SUBGRID_TILE_SIDE, las_sw_n + sub_y * _SUBGRID_TILE_SIDE


# ── Grid reconstruction ────────────────────────────────────────────────────────

def load_grid(path: Path) -> dict:
    with open(path) as f:
        return json.load(f)


def _x_extents(grid: dict) -> tuple[int, int]:
    widths = grid["widths"]
    offsets = grid["offsets"]
    x_min = min(o for o, w in zip(offsets, widths) if w > 0)
    x_max = max(o + w for o, w in zip(offsets, widths) if w > 0)
    return x_min, x_max


def _build_canvas(packed: np.ndarray, widths: list, offsets: list, x_min: int, x_max: int, extra_dims=()):
    """Place packed row data onto a spatially-correct canvas.

    packed[i, 0:widths[i]] is the valid data for row i, starting at global x = offsets[i].
    Returns (canvas, mask), both shape (nrows, canvas_w, *extra_dims), north-up (flipud applied).
    """
    nrows = packed.shape[0]
    canvas_w = x_max - x_min
    shape = (nrows, canvas_w) + extra_dims
    canvas = np.zeros(shape, dtype=packed.dtype)
    mask = np.ones(shape, dtype=bool)
    for i, (w, o) in enumerate(zip(widths, offsets)):
        if w > 0:
            col = o - x_min
            canvas[i, col : col + w] = packed[i, 0:w]
            mask[i, col : col + w] = False
    return np.flipud(canvas), np.flipud(mask)


def to_dense(grid: dict, dtype=float) -> np.ma.MaskedArray:
    """Reconstruct a spatially-correct dense 2D masked array from the stairstep JSON format.

    StairStepGrid layout:
      - values[i][0 : widths[i]] holds the valid data for row i.
      - offsets[i] gives the x position of values[i][0] in a shared coordinate space.
    """
    packed = np.array(grid["values"], dtype=dtype)
    widths = grid["widths"]
    offsets = grid["offsets"]
    x_min, x_max = _x_extents(grid)
    canvas, mask = _build_canvas(packed, widths, offsets, x_min, x_max)
    return np.ma.MaskedArray(canvas, mask)


def zone_to_dense_pair(grid: dict) -> tuple:
    """Return (bottom, top) as a pair of masked arrays (values in inches).

    The zone values array has shape (nrows, ncols, 2) where [..., 0]=bottom, [..., 1]=top.
    """
    packed = np.array(grid["values"], dtype=float)  # shape (nrows, ncols, 2)
    widths = grid["widths"]
    offsets = grid["offsets"]
    x_min, x_max = _x_extents(grid)
    canvas, mask = _build_canvas(packed, widths, offsets, x_min, x_max, extra_dims=(2,))
    bottom_ma = np.ma.MaskedArray(canvas[..., 0], mask[..., 0])
    top_ma = np.ma.MaskedArray(canvas[..., 1], mask[..., 1])
    return bottom_ma, top_ma


# ── Tile zoom ──────────────────────────────────────────────────────────────────

def tile_axes_limits(tile_str: str, grid: dict) -> tuple:
    """Return (xlim, ylim) in canvas-pixel coordinates for the given tile.

    xlim = (col_left, col_right)
    ylim = (row_bottom, row_top)  — note: larger row = more southern in the
                                    flipped image, so ylim[0] > ylim[1].
    """
    sw_e, sw_n = parse_tile_sw_corner(tile_str)
    ne_e = sw_e + _SUBGRID_TILE_SIDE
    ne_n = sw_n + _SUBGRID_TILE_SIDE

    e0, n0 = grid["base_offset"]
    nrows = len(grid["widths"])
    x_min, _ = _x_extents(grid)

    col_sw = sw_e - e0 - x_min
    col_ne = ne_e - e0 - x_min

    # After flipud, row index in the image for northing n is: nrows - 1 - (n - n0)
    row_sw = nrows - 1 - (sw_n - n0)   # SW = southern edge = larger image row
    row_ne = nrows - 1 - (ne_n - n0)   # NE = northern edge = smaller image row

    xlim = (col_sw - 0.5, col_ne + 0.5)
    ylim = (row_sw + 0.5, row_ne - 0.5)  # inverted: bottom > top in imshow coords
    return xlim, ylim


# ── Plotting ───────────────────────────────────────────────────────────────────

def plot_grid(ax, data: np.ma.MaskedArray, title: str, cmap: str, vmin=None, vmax=None):
    im = ax.imshow(
        data,
        aspect="auto",
        cmap=cmap,
        vmin=vmin,
        vmax=vmax,
        interpolation="nearest",
    )
    ax.set_title(title, fontsize=10)
    ax.set_xlabel("Easting (ft)")
    ax.set_ylabel("Northing (ft, S→N)")
    plt.colorbar(im, ax=ax, fraction=0.046, pad=0.04)


def main():
    parser = argparse.ArgumentParser(description="Visualize LOS stairstep debug dumps")
    parser.add_argument("dump_dir", help="Per-analysis debug dump directory")
    parser.add_argument("--tile", metavar="TILE_ID", help="Zoom all plots to this tile, e.g. 987200_10")
    args = parser.parse_args()

    dump_dir = Path(args.dump_dir)
    if not dump_dir.is_dir():
        parser.error(f"{dump_dir} is not a directory")

    files = {
        "zone_full": dump_dir / "zone_full.json",
        "zone_inner": dump_dir / "zone_inner.json",
        "terrain_full": dump_dir / "terrain_full.json",
        "terrain_inner": dump_dir / "terrain_inner.json",
        "intersection_full": dump_dir / "intersection_full.json",
        "intersection_inner": dump_dir / "intersection_inner.json",
    }

    missing = [k for k, v in files.items() if not v.exists()]
    if missing:
        parser.error(f"Missing files in dump dir: {missing}")

    grids = {k: load_grid(v) for k, v in files.items()}

    zone_full_bottom, zone_full_top = zone_to_dense_pair(grids["zone_full"])
    zone_inner_bottom, zone_inner_top = zone_to_dense_pair(grids["zone_inner"])
    terrain_full = to_dense(grids["terrain_full"], dtype=float)
    terrain_inner = to_dense(grids["terrain_inner"], dtype=float)
    intersection_full = to_dense(grids["intersection_full"], dtype=float)
    intersection_inner = to_dense(grids["intersection_inner"], dtype=float)

    def to_ft(arr):
        return arr / 12.0

    zone_full_bottom_ft = to_ft(zone_full_bottom)
    zone_inner_bottom_ft = to_ft(zone_inner_bottom)
    terrain_full_ft = to_ft(terrain_full)
    terrain_inner_ft = to_ft(terrain_inner)

    zone_bottom_max_ft = max(zone_full_bottom_ft.max(), zone_inner_bottom_ft.max())
    terrain_max_ft = max(terrain_full_ft.max(), terrain_inner_ft.max())

    # Layout: rows = (zone, terrain, occlusion), cols = (full, inner)
    fig, axes = plt.subplots(3, 2, figsize=(14, 15))
    title = f"Stairstep debug dump — {dump_dir.name}"
    if args.tile:
        title += f"\nzoomed to tile {args.tile}"
    fig.suptitle(title, fontsize=12)

    plot_grid(axes[0, 0], zone_full_bottom_ft, "Zone bottom (full) — ft", "viridis", vmin=0, vmax=zone_bottom_max_ft)
    plot_grid(axes[0, 1], zone_inner_bottom_ft, "Zone bottom (inner) — ft", "viridis", vmin=0, vmax=zone_bottom_max_ft)
    plot_grid(axes[1, 0], terrain_full_ft, "Terrain (full zone) — ft", "terrain", vmin=0, vmax=terrain_max_ft)
    plot_grid(axes[1, 1], terrain_inner_ft, "Terrain (inner zone) — ft", "terrain", vmin=0, vmax=terrain_max_ft)
    plot_grid(axes[2, 0], intersection_full, "Occlusion (full zone) — 0=clear 1=blocked", "RdYlGn_r", vmin=0, vmax=1)
    plot_grid(axes[2, 1], intersection_inner, "Occlusion (inner zone) — 0=clear 1=blocked", "RdYlGn_r", vmin=0, vmax=1)

    if args.tile:
        try:
            xlim, ylim = tile_axes_limits(args.tile, grids["terrain_full"])
        except (ValueError, KeyError) as e:
            parser.error(f"Could not parse tile id '{args.tile}': {e}")
        for ax in axes.flat:
            ax.set_xlim(xlim)
            ax.set_ylim(ylim)

    plt.tight_layout()
    plt.show()


if __name__ == "__main__":
    main()
