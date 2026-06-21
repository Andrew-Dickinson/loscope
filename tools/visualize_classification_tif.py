#!/usr/bin/env python3
"""
Visualize terrain classification TIFF files.

Usage:
    python visualize_classification_tif.py <tif_file> [<tif_file> ...]

Each file is shown as a subplot with a discrete colormap matching TerrainClass:
    0 = None        (grey)
    1 = Vegetation  (green)
    2 = Building    (red)
    3 = Water       (blue)

Requires: numpy, matplotlib, Pillow
"""

import argparse
import math
from pathlib import Path

import numpy as np
import matplotlib.pyplot as plt
import matplotlib.colors as mcolors
import matplotlib.patches as mpatches
from PIL import Image

# ── Classification palette (mirrors TerrainClass in types/tiles.rs) ───────────

CLASSES = {
    0: ("None",       "#888888"),
    1: ("Vegetation", "#4caf50"),
    2: ("Building",   "#e53935"),
    3: ("Water",      "#1e88e5"),
}

_CMAP = mcolors.ListedColormap([CLASSES[k][1] for k in sorted(CLASSES)])
_NORM = mcolors.BoundaryNorm(boundaries=[0, 1, 2, 3, 4], ncolors=4)

LEGEND_PATCHES = [
    mpatches.Patch(color=color, label=label)
    for _, (label, color) in sorted(CLASSES.items())
]


def load_tif(path: Path) -> np.ndarray:
    return np.array(Image.open(path), dtype=np.uint8)


def plot_tile(ax, arr: np.ndarray, title: str):
    ax.imshow(arr, cmap=_CMAP, norm=_NORM, interpolation="nearest", origin="upper")
    ax.set_title(title, fontsize=9)
    ax.axis("off")


def main():
    parser = argparse.ArgumentParser(description="Visualize terrain classification TIFFs")
    parser.add_argument("tifs", nargs="+", metavar="TIF", help="One or more .tif files")
    args = parser.parse_args()

    paths = [Path(p) for p in args.tifs]
    missing = [p for p in paths if not p.exists()]
    if missing:
        parser.error(f"File(s) not found: {missing}")

    n = len(paths)
    ncols = min(n, 4)
    nrows = math.ceil(n / ncols)

    fig, axes = plt.subplots(nrows, ncols, figsize=(4 * ncols, 4 * nrows + 0.6),
                             squeeze=False)
    fig.suptitle("Terrain classification", fontsize=12)

    for i, path in enumerate(paths):
        ax = axes[i // ncols][i % ncols]
        plot_tile(ax, load_tif(path), path.stem)

    for i in range(n, nrows * ncols):
        axes[i // ncols][i % ncols].set_visible(False)

    fig.legend(handles=LEGEND_PATCHES, loc="lower center", ncol=len(CLASSES),
               frameon=False, fontsize=10, bbox_to_anchor=(0.5, 0))
    plt.tight_layout(rect=[0, 0.05, 1, 1])
    plt.show()


if __name__ == "__main__":
    main()
