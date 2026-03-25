"""Compute the maximum ground elevation within a WKT polygon from DEM tiles.

DEM tiles use the same .tif + .json format as the LiDAR preprocessor:
  - <tile_id>.tif  — 500×500 uint16 raster, axes [easting_local, northing_local], inches
  - <tile_id>.json — {"tile_id": ..., "x_offset": e_sw, "y_offset": n_sw, ...}

Tiles are fetched lazily from S3 when not already cached locally.
Set these environment variables for remote access:
  LOS_DEM_S3_BUCKET   — S3 bucket name
  LOS_DEM_S3_PREFIX   — key prefix, e.g. "nyc-dem-2021/tiles"

CLI usage:
    python tools/max_ground_elevation.py "<WKT>" [--cache-dir DIR]
    echo "<WKT>" | python tools/max_ground_elevation.py - [--cache-dir DIR]
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import numpy as np
import pyproj
import shapely
import shapely.ops
import tifffile
from shapely import wkt

def _build_dem_fetcher(cache_dir: Path):
    """Return a CachingTileFetcher for DEM tiles backed by S3, or None if not configured.

    Reads LOS_DEM_S3_BUCKET and LOS_DEM_S3_PREFIX from the environment.
    """
    bucket = os.environ.get("LOS_DEM_S3_BUCKET")
    prefix = os.environ.get("LOS_DEM_S3_PREFIX")
    if not bucket or not prefix:
        return None
    from lib.tiles.fetch import CachingTileFetcher
    from lib.tiles.s3_backend import S3TileBackend
    return CachingTileFetcher(S3TileBackend(bucket, prefix), cache_dir)


def max_ground_elevation(wkt_str: str, cache_dir: Path) -> float | None:
    """Return the maximum ground elevation (in feet) within the WKT polygon.

    The input geometry must be in EPSG:6539.  DEM tiles are loaded from
    cache_dir, fetching any missing ones from S3 when configured.

    Returns None if no DEM data covers the polygon.
    """
    # Reuse _intersecting_tile_ids from building_footprints to find overlapping tiles
    from lib.obstructions.building_footprints import _intersecting_tile_ids

    poly_nys = wkt.loads(wkt_str)
    if poly_nys.is_empty:
        print("ERROR: geometry is empty after projection", file=sys.stderr)
        return None

    tile_ids = _intersecting_tile_ids(poly_nys)
    if not tile_ids:
        print("WARNING: no canonical tiles found for this polygon", file=sys.stderr)
        return None

    print(f"Polygon intersects {len(tile_ids)} tile(s): {tile_ids}", file=sys.stderr)

    # Lazily fetch tiles from S3 when not already cached locally
    fetcher = _build_dem_fetcher(cache_dir)
    if fetcher is not None:
        missing = [t for t in tile_ids if not fetcher.is_cached(t)]
        if missing:
            print(f"Fetching {len(missing)} tile(s) from S3 ...", file=sys.stderr)
            fetcher.ensure_tiles(missing)
        else:
            print("All tiles already cached.", file=sys.stderr)
    else:
        print(
            "S3 not configured (LOS_DEM_S3_BUCKET / LOS_DEM_S3_PREFIX not set)"
            " — using local tiles only.",
            file=sys.stderr,
        )

    max_inches: int | None = None

    for tile_id in tile_ids:
        tif_path  = cache_dir / f"{tile_id}.tif"
        json_path = cache_dir / f"{tile_id}.json"

        if not tif_path.exists() or not json_path.exists():
            print(f"  Tile {tile_id}: not found locally, skipping.", file=sys.stderr)
            continue

        meta = json.loads(json_path.read_text())
        e_sw = int(meta["x_offset"])
        n_sw = int(meta["y_offset"])

        # Raster axes: [easting_local, northing_local], values in uint16 inches
        raster = tifffile.imread(str(tif_path))
        E, N = raster.shape

        # Pixel [i, j] covers the 1-usft square whose center is at
        # easting (e_sw + i + 0.5), northing (n_sw + j + 0.5)
        xs = np.arange(E, dtype=np.float64) + e_sw + 0.5
        ys = np.arange(N, dtype=np.float64) + n_sw + 0.5
        xx, yy = np.meshgrid(xs, ys, indexing="ij")  # shape (E, N)

        inside = shapely.contains_xy(poly_nys, xx.ravel(), yy.ravel()).reshape(E, N)
        if not inside.any():
            continue

        tile_max = int(raster[inside].max())
        print(
            f"  Tile {tile_id}: max inside = {tile_max} in ({tile_max / 12:.2f} ft)",
            file=sys.stderr,
        )

        if max_inches is None or tile_max > max_inches:
            max_inches = tile_max

    if max_inches is None:
        print("WARNING: no DEM data found inside polygon.", file=sys.stderr)
        return None

    return max_inches / 12.0


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Compute maximum ground elevation within a WKT polygon from DEM tiles"
    )
    parser.add_argument(
        "wkt",
        help='WKT geometry string (POLYGON / MULTIPOLYGON in NYS LI SP), or "-" to read from stdin',
    )
    parser.add_argument(
        "--cache-dir",
        default="data/dem_tiles",
        metavar="DIR",
        help="Local cache directory for DEM tiles (default: data/dem_tiles)",
    )
    args = parser.parse_args()

    wkt_str = sys.stdin.read().strip() if args.wkt == "-" else args.wkt
    cache_dir = Path(args.cache_dir)
    cache_dir.mkdir(parents=True, exist_ok=True)

    result = max_ground_elevation(wkt_str, cache_dir)
    if result is None:
        sys.exit(1)

    print(f"{result:.4f} ft")


if __name__ == "__main__":
    main()
