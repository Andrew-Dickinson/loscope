#!/usr/bin/env python3
"""Export a preprocessed tile as a Blender-ready terrain mesh OBJ.

Usage:
    python tools/tile_to_obj.py <tile_id> [--in-dir data/preprocessed] [--out-dir data/obj]
                                          [--obs-dir data/obstructions]

Example:
    python tools/tile_to_obj.py 5247_10
    → writes data/obj/5247_10.obj           (terrain mesh)
    → writes data/obj/<obs_id>.obj  ...     (one file per obstruction, if any)

Coordinate system in both OBJ files:
    X = easting (local, ft)   — west→east
    Y = northing (local, ft)  — south→north
    Z = elevation (ft)

Origin is the SW corner of the tile. Import both files into the same Blender scene
and they will align correctly (File > Import > Wavefront (.obj) for each).

1 Blender unit = 1 US survey foot.
"""

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import tifffile
from tqdm import tqdm

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.lib.preprocessing.io import load_tile
from src.lib.preprocessing.tile_id import TILE_SIDE_USFT


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


def _write_obstruction_obj(
    f,
    vi: int,
    meta: dict,
    raster: np.ndarray,
    tile_x_offset: int,
    tile_y_offset: int,
) -> tuple[int, int]:
    """Write one obstruction as a named object into an already-open OBJ file.

    Returns (vi, cell_count) — updated vertex index and number of cells emitted.
    """
    obs_id = meta["obstruction_id"]
    local_x = int(meta["x_offset"]) - tile_x_offset
    local_y = int(meta["y_offset"]) - tile_y_offset

    obj_name = f"obs_{obs_id.replace('-', '_')}"
    f.write(f"o {obj_name}\n\n")

    W, H = raster.shape  # [easting_local, northing_local]
    cell_count = 0

    # Clamp iteration to cells that fall within the tile boundary.
    xi_lo = max(0, -local_x)
    xi_hi = min(W, TILE_SIDE_USFT - local_x)
    yi_lo = max(0, -local_y)
    yi_hi = min(H, TILE_SIDE_USFT - local_y)

    for xi in range(xi_lo, xi_hi):
        for yi in range(yi_lo, yi_hi):
            val = int(raster[xi, yi])
            if val == 0:
                continue

            zt = val / 12.0
            x0 = float(local_x + xi)
            y0 = float(local_y + yi)
            x1 = x0 + 1.0
            y1 = y0 + 1.0

            # Flat top face (CCW, normal +Z).
            f.write(f"v {x0} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y1} {zt:.3f}\n")
            f.write(f"v {x0} {y1} {zt:.3f}\n")
            o = vi
            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            # Side walls: only where this cell is taller than its neighbour.
            # Neighbours outside the raster OR outside the tile boundary both
            # count as nz=0 — the wall closes down to the ground plane.
            for dxi, dyi, ax, ay, bx, by in [
                ( 0, -1, x0, y0, x1, y0),  # south (-Y)
                ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                (+1,  0, x1, y0, x1, y1),  # east  (+X)
                (-1,  0, x0, y1, x0, y0),  # west  (-X)
            ]:
                nxi, nyi = xi + dxi, yi + dyi
                if xi_lo <= nxi < xi_hi and yi_lo <= nyi < yi_hi:
                    nval = int(raster[nxi, nyi])
                    nz = nval / 12.0 if nval > 0 else 0.0
                else:
                    nz = 0.0

                if zt > nz:
                    f.write(f"v {ax} {ay} {nz:.3f}\n")
                    f.write(f"v {bx} {by} {nz:.3f}\n")
                    f.write(f"v {bx} {by} {zt:.3f}\n")
                    f.write(f"v {ax} {ay} {zt:.3f}\n")
                    o = vi
                    f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

            cell_count += 1

    f.write("\n")
    return vi, cell_count


def export_obstructions_obj(
    tile_id: str,
    tile_x_offset: int,
    tile_y_offset: int,
    obs_dir: Path,
    out_dir: Path,
) -> int:
    """Write one OBJ per obstruction that overlaps this tile.

    Each file is named <obs_id>.obj and written to out_dir.  The coordinate
    origin matches the terrain OBJ (SW corner of the tile), so all files can be
    imported into the same Blender scene and will align correctly.

    Returns the number of OBJ files written.
    """
    obs_dir = Path(obs_dir)
    out_dir = Path(out_dir)

    # Collect all obstruction JSONs that list this tile_id.
    obs_entries: list[dict] = []
    for json_path in sorted(obs_dir.glob("*.json")):
        try:
            meta = json.loads(json_path.read_text())
        except (ValueError, OSError):
            continue
        if tile_id in meta.get("tile_ids", []):
            obs_entries.append(meta)

    if not obs_entries:
        return 0

    out_dir.mkdir(parents=True, exist_ok=True)
    n_written = 0

    for meta in tqdm(obs_entries, desc="Writing obstructions", unit="obs"):
        obs_id = meta["obstruction_id"]
        tif_path = obs_dir / meta.get("raster_file", f"{obs_id}.tif")
        try:
            raster = tifffile.imread(str(tif_path))
        except OSError:
            continue

        out_path = out_dir / f"{tile_id}_{obs_id}.obj"
        with open(out_path, "w") as f:
            f.write(f"# Obstruction {obs_id}\n")
            f.write(f"# Tile: {tile_id}\n")
            f.write("# 1 unit = 1 US survey foot\n")
            f.write("# X = easting (local), Y = northing (local), Z = elevation\n")
            f.write("# Origin = SW corner of the tile — aligns with the terrain OBJ\n\n")

            _, cell_count = _write_obstruction_obj(
                f, 1, meta, raster, tile_x_offset, tile_y_offset,
            )

        if cell_count:
            n_written += 1
        else:
            out_path.unlink(missing_ok=True)

    return n_written


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
    parser.add_argument(
        "--obs-dir",
        default="data/obstructions",
        help="Directory containing obstruction .tif/.json pairs (default: data/obstructions)",
    )
    args = parser.parse_args()

    print(f"Loading tile {args.tile_id!r} from {args.in_dir!r}…")
    tile = load_tile(args.tile_id, args.in_dir)

    out_path = Path(args.out_dir) / f"{args.tile_id}.obj"
    export_terrain_obj(tile.raster, out_path)

    print(f"\nExporting obstructions from {args.obs_dir!r}…")
    n = export_obstructions_obj(
        args.tile_id, tile.x_offset, tile.y_offset,
        Path(args.obs_dir), Path(args.out_dir),
    )
    if n:
        print(f"  {n} obstruction OBJ(s) written to {args.out_dir!r}")
    else:
        print("  No obstructions found for this tile.")


if __name__ == "__main__":
    main()
