"""Extract a heightmap and footprint mask for a given BIN from preprocessed LiDAR tiles.

Queries the building_footprints table for the given BIN (geometry stored in
EPSG:6539), fetches the required preprocessed LiDAR tiles, and writes two output
TIFF files:

    {out_dir}/{bin}_heightmap.tif  — uint16 heights in inches (1 usft/pixel grid)
    {out_dir}/{bin}_mask.tif       — uint8 mask: 1 inside footprint, 0 outside

Optionally generates a grid of 3D sample points (--sample-spacing):

    {out_dir}/{bin}_sample_points.npy — float64 (N, 3) array [easting, northing, z_feet]

The output grid's SW corner is at (floor(min_easting), floor(min_northing)) in NYS
integer coordinates.  Shape is (W, H) with axes [easting_local, northing_local],
matching the convention used by the preprocessed tile rasters.

Usage:
    python tools/extract_building_heightmap.py BIN [options]

Environment variables (optional — needed only when tiles are not already cached):
    LOS_S3_BUCKET   S3 bucket containing preprocessed tiles
    LOS_S3_PREFIX   Key prefix, e.g. "nyc-lidar-2021/preprocessed"
"""
from __future__ import annotations

import argparse
import dataclasses
from pathlib import Path

import numpy as np
import tifffile

from los_analyzer.building.heightmap import (
    build_building_heightmap,
    fetch_building_geometry,
    filter_heightmap_outliers,
)
from los_analyzer.sample_points import apply_mast_offset, generate_sample_points


def _build_fetcher(tile_dir: Path):
    """Return a CachingTileFetcher backed by S3, or None if not configured."""
    import os
    bucket = os.environ.get("LOS_S3_BUCKET")
    prefix = os.environ.get("LOS_S3_PREFIX")
    if not bucket or not prefix:
        return None
    from los_analyzer.tiles.fetch import CachingTileFetcher
    from los_analyzer.tiles.s3_backend import S3TileBackend
    return CachingTileFetcher(S3TileBackend(bucket, prefix), tile_dir)



_CUBE_HALF = 0.125  # 3 inches = 0.25 ft; half-edge = 0.125 ft


@dataclasses.dataclass
class ExtractionResult:
    heightmap_path: Path
    mask_path: Path
    sample_pts_path: Path | None = None
    sample_pts_measurement_path: Path | None = None
    terrain_obj_path: Path | None = None
    sample_pts_display_obj_path: Path | None = None
    sample_pts_measurement_obj_path: Path | None = None


def _export_heightmap_obj(heightmap: np.ndarray, out_path: Path) -> None:
    """Write a Minecraft-style voxel terrain OBJ from a building heightmap.

    Matches the style of ``tile_to_obj.py``: flat top faces with vertical walls
    wherever a cell is taller than its neighbour.

    Local coordinates: pixel ``[xi, yi]`` → ``X ∈ [xi, xi+1)``, ``Y ∈ [yi, yi+1)``.
    Origin is the SW corner of the heightmap (aligns with the sample-points OBJ).
    """
    z = heightmap.astype(np.float32) / 12.0
    W, H = z.shape
    z_floor = 0.0

    out_path.parent.mkdir(parents=True, exist_ok=True)
    vi = 1

    with open(out_path, "w") as f:
        f.write(f"# Building heightmap terrain — {W}×{H} raster cells\n")
        f.write("# 1 unit = 1 US survey foot\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
        f.write("# Origin = SW corner of the heightmap\n\n")
        f.write("o heightmap\n\n")

        # Single bottom cap at z_floor.
        f.write(f"v 0 0 {z_floor:.3f}\n")
        f.write(f"v {W} 0 {z_floor:.3f}\n")
        f.write(f"v {W} {H} {z_floor:.3f}\n")
        f.write(f"v 0 {H} {z_floor:.3f}\n")
        f.write(f"f {vi} {vi+1} {vi+2} {vi+3}\n\n")
        vi += 4

        for xi in range(W):
            for yi in range(H):
                zt = float(z[xi, yi])
                x0, y0 = float(xi), float(yi)
                x1, y1 = x0 + 1.0, y0 + 1.0

                # Flat top face (CCW, normal +Z).
                f.write(f"v {x0} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y0} {zt:.3f}\n")
                f.write(f"v {x1} {y1} {zt:.3f}\n")
                f.write(f"v {x0} {y1} {zt:.3f}\n")
                o = vi
                f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                vi += 4

                # Vertical walls wherever this cell is taller than its neighbour.
                for dxi, dyi, ax, ay, bx, by in [
                    ( 0, -1, x0, y0, x1, y0),  # south (−Y)
                    ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                    (+1,  0, x1, y0, x1, y1),  # east  (+X)
                    (-1,  0, x0, y1, x0, y0),  # west  (−X)
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


def _export_sample_points_obj(
    pts: np.ndarray,
    x_sw: int,
    y_sw: int,
    out_path: Path,
) -> None:
    """Write sample points as 3-inch cubes to an OBJ file.

    *pts* is an ``(N, 3)`` float64 array ``[easting, northing, z_feet]`` in
    absolute EPSG:6539 coordinates.  Coordinates are converted to local
    (relative to *x_sw*, *y_sw*) so the file aligns with the heightmap OBJ.

    Each point becomes a closed box with edge length 3 inches (0.25 ft).
    Faces are written with four dedicated vertices each (same style as
    ``tile_to_obj.py``) — six faces × four vertices = 24 vertices per cube.
    """
    h = _CUBE_HALF
    out_path.parent.mkdir(parents=True, exist_ok=True)
    vi = 1

    with open(out_path, "w") as f:
        f.write(f"# Sample points — {len(pts)} points, each a 3-inch cube\n")
        f.write("# 1 unit = 1 US survey foot\n")
        f.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
        f.write("# Origin = SW corner of the heightmap — aligns with the terrain OBJ\n\n")
        f.write("o sample_points\n\n")

        for x_abs, y_abs, z in pts:
            cx = float(x_abs) - x_sw
            cy = float(y_abs) - y_sw
            cz = float(z)

            # Six outward faces, each defined by its four vertices in order.
            # Winding is CCW when viewed from outside.
            faces = (
                # top    (+Z normal)
                ((cx-h, cy-h, cz+h), (cx+h, cy-h, cz+h), (cx+h, cy+h, cz+h), (cx-h, cy+h, cz+h)),
                # bottom (−Z normal)
                ((cx-h, cy+h, cz-h), (cx+h, cy+h, cz-h), (cx+h, cy-h, cz-h), (cx-h, cy-h, cz-h)),
                # south  (−Y normal)
                ((cx-h, cy-h, cz-h), (cx+h, cy-h, cz-h), (cx+h, cy-h, cz+h), (cx-h, cy-h, cz+h)),
                # north  (+Y normal)
                ((cx+h, cy+h, cz-h), (cx-h, cy+h, cz-h), (cx-h, cy+h, cz+h), (cx+h, cy+h, cz+h)),
                # east   (+X normal)
                ((cx+h, cy-h, cz-h), (cx+h, cy+h, cz-h), (cx+h, cy+h, cz+h), (cx+h, cy-h, cz+h)),
                # west   (−X normal)
                ((cx-h, cy+h, cz-h), (cx-h, cy-h, cz-h), (cx-h, cy-h, cz+h), (cx-h, cy+h, cz+h)),
            )
            for verts in faces:
                for vx, vy, vz in verts:
                    f.write(f"v {vx:.4f} {vy:.4f} {vz:.4f}\n")
                o = vi
                f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                vi += 4


def extract_building_heightmap(
    bin_id: str,
    db_path: Path,
    tile_dir: Path,
    out_dir: Path,
    sample_spacing: int | None = None,
    mast_offset: float = 0.0,
    export_obj: bool = False,
) -> ExtractionResult:
    """Extract heightmap and mask TIFFs for the given BIN.

    When *sample_spacing* is given (integer feet, >= 1), generates sample
    points and applies *mast_offset* to produce separate display and
    measurement positions.  For every point that is the highest at its (X, Y),
    the measurement position is shifted upward by *mast_offset* feet; all
    other points have coincident display and measurement positions.

    Saves ``{bin_id}_sample_points.npy`` (display) and
    ``{bin_id}_sample_points_measurement.npy`` (measurement).

    When *export_obj* is ``True``, writes ``{bin_id}_heightmap.obj`` plus
    ``{bin_id}_sample_points_display.obj`` and
    ``{bin_id}_sample_points_measurement.obj`` (each point a 3-inch cube).
    All OBJ files share the same local coordinate origin so they align in
    Blender.
    """
    # 1. Optionally prefetch tiles from S3 before the main extraction
    from los_analyzer.obstructions.building_footprints import _intersecting_tile_ids
    poly_for_fetch = fetch_building_geometry(bin_id, db_path)
    tile_ids_for_fetch = _intersecting_tile_ids(poly_for_fetch)

    fetcher = _build_fetcher(tile_dir)
    if fetcher is not None:
        missing = [t for t in tile_ids_for_fetch if not fetcher.is_cached(t)]
        if missing:
            print(f"Fetching {len(missing)} tile(s) from S3: {missing}")
            fetcher.ensure_tiles(missing)
    else:
        print("S3 not configured — using local tiles only.")

    # 2. Build heightmap + mask via shared library
    heightmap, mask, poly_nys, x_sw, y_sw, found_tiles = build_building_heightmap(
        bin_id, db_path, tile_dir
    )
    heightmap = filter_heightmap_outliers(heightmap, mask)

    # 3. Write outputs
    out_dir.mkdir(parents=True, exist_ok=True)
    heightmap_path = out_dir / f"{bin_id}_heightmap.tif"
    mask_path = out_dir / f"{bin_id}_mask.tif"
    tifffile.imwrite(str(heightmap_path), heightmap)
    tifffile.imwrite(str(mask_path), mask)

    # 6. Optional sample-point grid
    sample_pts_path: Path | None = None
    sample_pts_measurement_path: Path | None = None
    display_pts: np.ndarray | None = None
    measurement_pts: np.ndarray | None = None
    if sample_spacing is not None:
        raw_pts = generate_sample_points(
            heightmap, x_sw, y_sw, sample_spacing,
            mask=mask, polygon=poly_nys,
        )
        display_pts, measurement_pts = apply_mast_offset(raw_pts, mast_offset)
        sample_pts_path = out_dir / f"{bin_id}_sample_points.npy"
        sample_pts_measurement_path = out_dir / f"{bin_id}_sample_points_measurement.npy"
        np.save(str(sample_pts_path), display_pts)
        np.save(str(sample_pts_measurement_path), measurement_pts)

    # 7. Optional OBJ visualisation
    terrain_obj_path: Path | None = None
    sample_pts_display_obj_path: Path | None = None
    sample_pts_measurement_obj_path: Path | None = None
    if export_obj:
        terrain_obj_path = out_dir / f"{bin_id}_heightmap.obj"
        _export_heightmap_obj(heightmap, terrain_obj_path)
        if display_pts is not None:
            sample_pts_display_obj_path = out_dir / f"{bin_id}_sample_points_display.obj"
            sample_pts_measurement_obj_path = out_dir / f"{bin_id}_sample_points_measurement.obj"
            _export_sample_points_obj(display_pts, x_sw, y_sw, sample_pts_display_obj_path)
            _export_sample_points_obj(measurement_pts, x_sw, y_sw, sample_pts_measurement_obj_path)

    return ExtractionResult(
        heightmap_path=heightmap_path,
        mask_path=mask_path,
        sample_pts_path=sample_pts_path,
        sample_pts_measurement_path=sample_pts_measurement_path,
        terrain_obj_path=terrain_obj_path,
        sample_pts_display_obj_path=sample_pts_display_obj_path,
        sample_pts_measurement_obj_path=sample_pts_measurement_obj_path,
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Extract LiDAR heightmap and footprint mask TIFFs for a building BIN"
    )
    parser.add_argument("bin", metavar="BIN", help="Building Identification Number")
    parser.add_argument(
        "--db",
        default="data/nyc_dob.db",
        metavar="PATH",
        help="SQLite database path (default: data/nyc_dob.db)",
    )
    parser.add_argument(
        "--tile-dir",
        default="data/preprocessed",
        metavar="DIR",
        help="Preprocessed LiDAR tile directory (default: data/preprocessed)",
    )
    parser.add_argument(
        "--out-dir",
        default="data/building_heightmaps",
        metavar="DIR",
        help="Output directory for TIFF files (default: data/building_heightmaps)",
    )
    parser.add_argument(
        "--sample-spacing",
        type=int,
        default=None,
        metavar="FEET",
        help=(
            "If given, generate a 3D sample-point grid with this XY spacing "
            "(integer feet, must be >= 1).  Saved as {BIN}_sample_points.npy."
        ),
    )
    parser.add_argument(
        "--mast-offset",
        type=float,
        default=0.0,
        metavar="FEET",
        help=(
            "Vertical offset in feet applied to the measurement position of "
            "the top point at each (X, Y) location (default: 0).  Display "
            "positions are always saved at the original surface height."
        ),
    )
    parser.add_argument(
        "--export-obj",
        action="store_true",
        help=(
            "Write Minecraft-style OBJ files: {BIN}_heightmap.obj (voxel "
            "terrain), {BIN}_sample_points_display.obj, and "
            "{BIN}_sample_points_measurement.obj (each point a 3-inch cube). "
            "All share the same local coordinate origin for Blender import."
        ),
    )
    args = parser.parse_args()

    if args.sample_spacing is not None and args.sample_spacing < 1:
        parser.error("--sample-spacing must be >= 1")

    db_path = Path(args.db)
    tile_dir = Path(args.tile_dir)
    out_dir = Path(args.out_dir)

    result = extract_building_heightmap(
        args.bin, db_path, tile_dir, out_dir,
        sample_spacing=args.sample_spacing,
        mast_offset=args.mast_offset,
        export_obj=args.export_obj,
    )
    print(f"Heightmap:  {result.heightmap_path}")
    print(f"Mask:       {result.mask_path}")
    if result.sample_pts_path is not None:
        pts = np.load(str(result.sample_pts_path))
        print(f"Display pts:     {result.sample_pts_path}  ({len(pts)} points)")
        print(f"Measurement pts: {result.sample_pts_measurement_path}")
    if result.terrain_obj_path is not None:
        print(f"Terrain OBJ:     {result.terrain_obj_path}")
    if result.sample_pts_display_obj_path is not None:
        print(f"Display OBJ:     {result.sample_pts_display_obj_path}")
        print(f"Measurement OBJ: {result.sample_pts_measurement_obj_path}")


if __name__ == "__main__":
    main()
