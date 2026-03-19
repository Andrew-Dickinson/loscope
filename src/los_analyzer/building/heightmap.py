"""Building heightmap extraction from preprocessed LiDAR tiles.

Provides reusable functions for querying a building's NYS geometry from a
SQLite database and blitting the corresponding LiDAR tile heights into a
dense grid aligned to the building footprint bounds.
"""
from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import Any

import numpy as np
import shapely
from shapely import wkt

from los_analyzer.obstructions.building_footprints import _intersecting_tile_ids
from los_analyzer.preprocessing.io import load_tile


def fetch_building_geometry(bin_id: str, db_path: Path) -> Any:
    """Return the NYS EPSG:6539 shapely geometry for the given BIN.

    Queries the building_footprints table in *db_path* for the row whose
    ``bin`` column equals *bin_id*.

    Raises:
        ValueError: If no matching row is found, the geometry field is empty,
            or the WKT cannot be parsed.
    """
    con = sqlite3.connect(str(db_path))
    con.execute("PRAGMA query_only=ON")
    row = con.execute(
        "SELECT the_geom FROM building_footprints WHERE bin = ? LIMIT 1",
        (bin_id,),
    ).fetchone()
    con.close()

    if row is None or not row[0]:
        raise ValueError(f"BIN {bin_id!r} not found in building_footprints")

    try:
        geom = wkt.loads(row[0])
    except Exception as exc:
        raise ValueError(f"Could not parse geometry for BIN {bin_id!r}: {exc}") from exc

    if geom.is_empty:
        raise ValueError(f"Empty geometry for BIN {bin_id!r}")

    return geom


def build_building_heightmap(
    bin_id: str,
    db_path: Path,
    tile_dir: Path,
) -> tuple[np.ndarray, np.ndarray, Any, int, int, list[str]]:
    """Build a dense heightmap and mask for the given BIN.

    Queries the building geometry, identifies the overlapping preprocessed
    LiDAR tiles, blits their height values into a dense (W, H) grid aligned
    to the building footprint bounds, and rasterizes the footprint mask.

    Args:
        bin_id: Building Identification Number to look up.
        db_path: Path to the SQLite database containing building_footprints.
        tile_dir: Directory containing preprocessed LiDAR tile .tif files.

    Returns:
        A 6-tuple ``(heightmap, mask, polygon, x_sw, y_sw, tile_ids)`` where:

        - ``heightmap``: ``uint16`` array ``(W, H)`` — height in inches,
          axes ``[easting_local, northing_local]``.  Pixels outside the
          buffered footprint are set to 0.
        - ``mask``: ``uint8`` array ``(W, H)`` — 255 inside the footprint
          (with 0.5 ft buffer), 0 outside.
        - ``polygon``: Shapely geometry in NYS EPSG:6539.
        - ``x_sw``: SW-corner easting of the output grid (integer usft).
        - ``y_sw``: SW-corner northing of the output grid (integer usft).
        - ``tile_ids``: List of tile IDs that were found and blitted.

    Raises:
        ValueError: If the BIN is not found, has empty geometry, or no
            tiles intersect the footprint bounds.
    """
    # 1. Fetch boundary (already in NYS EPSG:6539)
    poly_nys = fetch_building_geometry(bin_id, db_path)

    minx, miny, maxx, maxy = poly_nys.bounds
    x_sw = int(np.floor(minx))
    y_sw = int(np.floor(miny))
    x_ne = int(np.ceil(maxx))
    y_ne = int(np.ceil(maxy))
    W = max(x_ne - x_sw, 1)
    H = max(y_ne - y_sw, 1)

    # 2. Identify required tiles
    tile_ids = _intersecting_tile_ids(poly_nys)
    if not tile_ids:
        raise ValueError(f"No preprocessed tiles cover BIN {bin_id!r}")

    # 3. Blit tile heights into the output grid
    heightmap = np.zeros((W, H), dtype=np.uint16)
    found_tile_ids: list[str] = []

    for tile_id in tile_ids:
        tif_path = tile_dir / f"{tile_id}.tif"
        if not tif_path.exists():
            continue

        found_tile_ids.append(tile_id)
        tile = load_tile(tile_id, tile_dir)
        tile_w, tile_h = tile.raster.shape  # axes [easting_local, northing_local]

        # Overlap in easting
        e_start = max(x_sw, tile.x_offset)
        e_end = min(x_ne, tile.x_offset + tile_w)
        if e_start >= e_end:
            continue

        # Overlap in northing
        n_start = max(y_sw, tile.y_offset)
        n_end = min(y_ne, tile.y_offset + tile_h)
        if n_start >= n_end:
            continue

        # Indices into output grid
        out_e0 = e_start - x_sw
        out_e1 = e_end - x_sw
        out_n0 = n_start - y_sw
        out_n1 = n_end - y_sw

        # Indices into tile raster
        tile_e0 = e_start - tile.x_offset
        tile_e1 = e_end - tile.x_offset
        tile_n0 = n_start - tile.y_offset
        tile_n1 = n_end - tile.y_offset

        heightmap[out_e0:out_e1, out_n0:out_n1] = tile.raster[tile_e0:tile_e1, tile_n0:tile_n1]

    # 4. Rasterize footprint mask (pixel centres, matching _rasterize convention)
    xs = np.arange(W, dtype=np.float64) + x_sw + 0.5
    ys = np.arange(H, dtype=np.float64) + y_sw + 0.5
    xx, yy = np.meshgrid(xs, ys, indexing="ij")  # shape (W, H)
    # Buffer by half a pixel so that pixels the boundary crosses are included.
    inside = shapely.contains_xy(poly_nys.buffer(0.5), xx.ravel(), yy.ravel()).reshape(W, H)
    mask = np.where(inside, np.uint8(255), np.uint8(0))
    heightmap[~inside] = 0

    return heightmap, mask, poly_nys, x_sw, y_sw, found_tile_ids
