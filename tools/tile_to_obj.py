#!/usr/bin/env python3
"""Export a preprocessed tile as a Blender-ready terrain mesh OBJ.

Usage:
    python tools/tile_to_obj.py <tile_id> [--in-dir data/preprocessed] [--out-dir data/obj]

Example:
    python tools/tile_to_obj.py 5247_10
    → writes data/obj/5247_10.obj (500×500 flat quads, one per raster cell)

Coordinate system in the OBJ:
    X = easting (local, ft)   — west→east
    Y = northing (local, ft)  — south→north
    Z = elevation (ft)

1 Blender unit = 1 US survey foot.
Import in Blender: File > Import > Wavefront (.obj)
"""

import argparse
import sys
from pathlib import Path

import numpy as np
from tqdm import tqdm

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.los_analyzer.preprocessing.io import load_tile


def export_terrain_obj(raster_inches: np.ndarray, out_path: Path) -> None:
    """Write a voxel-style terrain OBJ from a (W, H) uint16 height raster (inches).

    Axes convention: raster[xi, yi] = height at easting xi, northing yi.

    Each cell gets a flat top face at its own height. Where a cell is taller
    than its neighbour, a vertical wall is emitted to close the gap — giving the
    "Minecraft" stepped look so every pixel's boundary is clearly visible.
    """
    z = raster_inches.astype(np.float32) / 12.0
    W, H = z.shape  # 500, 500 for standard tiles
    z_floor = float(z.min())

    out_path = Path(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    vi = 1  # OBJ vertex indices are 1-based

    with open(out_path, "w") as f:
        f.write(f"# LiDAR terrain mesh — {W}×{H} raster cells\n")
        f.write("# 1 Blender unit = 1 US survey foot\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
        f.write("# Import: File > Import > Wavefront (.obj)\n\n")
        f.write("o terrain\n\n")

        # Single bottom face at z_floor.
        f.write(f"v 0 0 {z_floor:.3f}\n")
        f.write(f"v {W} 0 {z_floor:.3f}\n")
        f.write(f"v {W} {H} {z_floor:.3f}\n")
        f.write(f"v 0 {H} {z_floor:.3f}\n")
        f.write(f"f {vi} {vi+1} {vi+2} {vi+3}\n\n")
        vi += 4

        with tqdm(total=W * H, desc="Writing cells", unit="cells") as pbar:
            for xi in range(W):
                for yi in range(H):
                    zt = float(z[xi, yi])
                    x0, y0 = float(xi), float(yi)
                    x1, y1 = x0 + 1.0, y0 + 1.0

                    # Flat top face (CCW winding, normal points +Z).
                    f.write(f"v {x0} {y0} {zt:.3f}\n")
                    f.write(f"v {x1} {y0} {zt:.3f}\n")
                    f.write(f"v {x1} {y1} {zt:.3f}\n")
                    f.write(f"v {x0} {y1} {zt:.3f}\n")
                    o = vi
                    f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

                    # Side walls: emit where this cell is taller than its neighbour.
                    # ax,ay → bx,by is the shared edge (bottom of wall = neighbour top).
                    for dxi, dyi, ax, ay, bx, by in [
                        ( 0, -1, x0, y0, x1, y0),  # south (-Y)
                        ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                        (+1,  0, x1, y0, x1, y1),  # east  (+X)
                        (-1,  0, x0, y1, x0, y0),  # west  (-X)
                    ]:
                        nxi, nyi = xi + dxi, yi + dyi
                        if 0 <= nxi < W and 0 <= nyi < H:
                            nz = float(z[nxi, nyi])
                            zb = nz
                        else:
                            nz = z_floor - 1.0  # sentinel: always emit at boundary
                            zb = z_floor

                        if zt > nz:
                            f.write(f"v {ax} {ay} {zb:.3f}\n")
                            f.write(f"v {bx} {by} {zb:.3f}\n")
                            f.write(f"v {bx} {by} {zt:.3f}\n")
                            f.write(f"v {ax} {ay} {zt:.3f}\n")
                            o = vi
                            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                            vi += 4

                    pbar.update(1)

    n_verts = vi - 1
    print(f"\nSaved: {out_path}\n  {W}×{H} cells · {n_verts:,} vertices")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Export a preprocessed LiDAR tile as a Blender terrain mesh OBJ."
    )
    parser.add_argument("tile_id", help="Tile ID, e.g. 5247_10")
    parser.add_argument(
        "--in-dir",
        default="data/preprocessed",
        help="Directory containing .tif/.json tile pairs (default: data/preprocessed)",
    )
    parser.add_argument(
        "--out-dir",
        default="data/obj",
        help="Output directory for .obj files (default: data/obj)",
    )
    args = parser.parse_args()

    print(f"Loading tile {args.tile_id!r} from {args.in_dir!r}…")
    tile = load_tile(args.tile_id, args.in_dir)

    out_path = Path(args.out_dir) / f"{args.tile_id}.obj"
    export_terrain_obj(tile.raster, out_path)


if __name__ == "__main__":
    main()
