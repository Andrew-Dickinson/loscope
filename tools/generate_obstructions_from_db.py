"""Generate obstruction tif + json pairs from SQL queries against data/nyc_dob.db.

Runs a single SQL query and writes one tif+json pair per result row.
Rows where ground_elevation is NULL use DEM tiles to derive the maximum ground
elevation within the polygon footprint.

The SQL query must return these columns:
    output_geometry  -- WKT polygon/multipolygon in WGS84 (EPSG:4326)
    ground_elevation -- absolute elevation in feet (may be NULL)
    height_roof      -- height above ground in feet
    type             -- obstruction type string (used as obstruction_type)
    props            -- JSON string of attributes to store verbatim

Usage:
    python tools/generate_obstructions_from_db.py QUERY [--db PATH]
        [--out-dir DIR] [--dem-cache DIR]
"""
from __future__ import annotations

import argparse
import json
import os
import sqlite3
import uuid
import warnings
from pathlib import Path

import numpy as np
import shapely
import tifffile
from shapely import wkt

from los_analyzer.obstructions.building_footprints import (
    _intersecting_tile_ids,
    _project_geometry,
    _rasterize,
)
from los_analyzer.obstructions.io import save_obstruction
from los_analyzer.obstructions.model import Obstruction


def _build_dem_fetcher(cache_dir: Path):
    """Return a CachingTileFetcher backed by S3, or None if not configured.

    Reads LOS_DEM_S3_BUCKET and LOS_DEM_S3_PREFIX from the environment.
    """
    bucket = os.environ.get("LOS_DEM_S3_BUCKET")
    prefix = os.environ.get("LOS_DEM_S3_PREFIX")
    if not bucket or not prefix:
        return None
    from los_analyzer.tiles.fetch import CachingTileFetcher
    from los_analyzer.tiles.s3_backend import S3TileBackend
    return CachingTileFetcher(S3TileBackend(bucket, prefix), cache_dir)


def _max_ground_elevation_from_dem(poly_nys, dem_cache: Path, fetcher=None) -> float | None:
    """Return max ground elevation (feet) inside poly_nys using local DEM tiles.

    poly_nys must already be projected to NYS EPSG:6539. Returns None when no
    DEM data covers the polygon.
    """
    tile_ids = _intersecting_tile_ids(poly_nys)
    if not tile_ids:
        return None

    if fetcher is not None:
        missing = [t for t in tile_ids if not fetcher.is_cached(t)]
        if missing:
            fetcher.ensure_tiles(missing)

    max_inches: int | None = None

    for tile_id in tile_ids:
        tif_path = dem_cache / f"{tile_id}.tif"
        json_path = dem_cache / f"{tile_id}.json"
        if not tif_path.exists() or not json_path.exists():
            continue

        meta = json.loads(json_path.read_text())
        e_sw = int(meta["x_offset"])
        n_sw = int(meta["y_offset"])

        # Raster axes: [easting_local, northing_local], values in uint16 inches
        raster = tifffile.imread(str(tif_path))
        E, N = raster.shape

        xs = np.arange(E, dtype=np.float64) + e_sw + 0.5
        ys = np.arange(N, dtype=np.float64) + n_sw + 0.5
        xx, yy = np.meshgrid(xs, ys, indexing="ij")  # shape (E, N)

        inside = shapely.contains_xy(poly_nys, xx.ravel(), yy.ravel()).reshape(E, N)
        if not inside.any():
            continue

        tile_max = int(raster[inside].max())
        if max_inches is None or tile_max > max_inches:
            max_inches = tile_max

    return max_inches / 12.0 if max_inches is not None else None


def process_query(sql_path: Path, db_path: Path, out_dir: Path, dem_cache: Path, fetcher=None) -> int:
    """Run one SQL query and write one tif+json pair per result row.

    Returns the number of obstruction files written.
    """
    from tqdm import tqdm

    sql = sql_path.read_text()
    con = sqlite3.connect(str(db_path))
    con.row_factory = sqlite3.Row
    con.execute("PRAGMA query_only=ON")
    con.execute("PRAGMA cache_size=-65536")   # 64 MB page cache
    con.execute("PRAGMA temp_store=MEMORY")

    with tqdm(desc="Running query", unit=" ops", bar_format="{desc}: {elapsed} [{n}{unit}]") as pbar:
        def _progress():
            pbar.update(100_000)

        con.set_progress_handler(_progress, 100_000)
        rows = con.execute(sql).fetchall()
        con.set_progress_handler(None, 0)

    written = 0
    skipped = 0

    for row in tqdm(rows, unit="row", desc=f"Generating obstruction files for {sql_path}"):
        geom_str = row["output_geometry"]
        if not geom_str:
            skipped += 1
            continue

        height_roof = row["height_roof"]
        if height_roof is None:
            skipped += 1
            continue

        try:
            poly_wgs84 = wkt.loads(geom_str)
        except Exception:
            skipped += 1
            continue

        poly_nys = _project_geometry(poly_wgs84)
        if poly_nys.is_empty:
            skipped += 1
            continue

        ground_elevation = row["ground_elevation"]
        props = json.loads(row["props"])

        if ground_elevation is None:
            ground_elevation = _max_ground_elevation_from_dem(poly_nys, dem_cache, fetcher)
            if ground_elevation is None:
                skipped += 1
                continue
            # Persist the resolved value in the stored attributes
            props["ground_elevation"] = ground_elevation

        total_height_ft = float(ground_elevation) + float(height_roof)
        height_inches = int(round(total_height_ft * 12))

        tile_ids = _intersecting_tile_ids(poly_nys)
        x_sw, y_sw, raster = _rasterize(poly_nys, height_inches)

        if not raster.any():
            skipped += 1
            continue

        obs = Obstruction(
            obstruction_id=str(uuid.uuid4()),
            obstruction_type=row["type"],
            attributes=props,
            x_offset=x_sw,
            y_offset=y_sw,
            raster=raster,
            tile_ids=tile_ids,
        )
        save_obstruction(obs, out_dir)
        written += 1

    con.close()

    if skipped:
        warnings.warn(f"{sql_path.name}: skipped {skipped} rows with missing/invalid data")

    return written


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate obstruction tif+json pairs from a SQL query against nyc_dob.db"
    )
    parser.add_argument(
        "query",
        metavar="QUERY",
        help="Path to a .sql file to run",
    )
    parser.add_argument(
        "--db",
        default="data/nyc_dob.db",
        metavar="PATH",
        help="Path to SQLite database (default: data/nyc_dob.db)",
    )
    parser.add_argument(
        "--out-dir",
        default="data/obstructions",
        metavar="DIR",
        help="Output directory for tif+json pairs (default: data/obstructions)",
    )
    parser.add_argument(
        "--dem-cache",
        default="data/dem_tiles",
        metavar="DIR",
        help="Local DEM tile cache for ground elevation lookup (default: data/dem_tiles)",
    )
    args = parser.parse_args()

    sql_path = Path(args.query)
    db_path = Path(args.db)
    out_dir = Path(args.out_dir)
    dem_cache = Path(args.dem_cache)

    out_dir.mkdir(parents=True, exist_ok=True)
    dem_cache.mkdir(parents=True, exist_ok=True)

    fetcher = _build_dem_fetcher(dem_cache)
    if fetcher is None:
        print(
            "S3 not configured (LOS_DEM_S3_BUCKET / LOS_DEM_S3_PREFIX not set)"
            " — using local DEM tiles only."
        )

    print(f"Processing {sql_path.name} ...")
    count = process_query(sql_path, db_path, out_dir, dem_cache, fetcher)
    print(f"Done. {count} obstruction(s) written.")


if __name__ == "__main__":
    main()
