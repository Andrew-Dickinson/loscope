"""Building heightmap extraction from preprocessed LiDAR tiles.

Provides reusable functions for querying a building's NYS geometry from a
SQLite database and blitting the corresponding LiDAR tile heights into a
dense grid aligned to the building footprint bounds.
"""
from __future__ import annotations

import dataclasses
import sqlite3
from pathlib import Path
from typing import NamedTuple, TypedDict

import numpy as np
import shapely
from shapely import wkt
from shapely.geometry.base import BaseGeometry

from los_analyzer.lib.obstructions.building_footprints import _intersecting_tile_ids
from los_analyzer.lib.providers.dob_db_dao import DOBDBDAO
from los_analyzer.lib.providers.tile_provider import TileProvider


# Precomputed circular neighbor offsets for each supported radius (in pixels).
# Excludes the centre pixel (self).
def _circular_offsets(radius: float) -> list[tuple[int, int]]:
    r = int(np.ceil(radius))
    return [
        (di, dj)
        for di in range(-r, r + 1)
        for dj in range(-r, r + 1)
        if 0 < di ** 2 + dj ** 2 <= radius ** 2
    ]


@dataclasses.dataclass
class RooftopHeightMap:
    bin_id: str
    x_sw: int
    y_sw: int
    heightmap: np.ndarray
    mask: np.ndarray
    poly_nys: BaseGeometry

def build_building_heightmap(
    bin_id: str,
    dob_db_dao: DOBDBDAO,
    tile_provider: TileProvider,
) -> RooftopHeightMap:
    """Build a dense heightmap and mask for the given BIN.

    Queries the building geometry, identifies the overlapping preprocessed
    LiDAR tiles, blits their height values into a dense (W, H) grid aligned
    to the building footprint bounds, and rasterizes the footprint mask.

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
    poly_nys = dob_db_dao.fetch_building_footprint_geometry(bin_id)

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

    for tile_id in tile_ids:
        tile = tile_provider.get_tile(tile_id)
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

    return RooftopHeightMap(
        bin_id=bin_id,
        x_sw=x_sw,
        y_sw=y_sw,
        heightmap=filter_heightmap_outliers(heightmap, mask),
        mask=mask,
        poly_nys=poly_nys
    )


def filter_heightmap_outliers(
    heightmap: np.ndarray,
    mask: np.ndarray,
    radius: float = 3.0,
    threshold_sigma: float = 3.0,
) -> np.ndarray:
    """Replace statistical outlier height pixels with the local neighbourhood median.

    For each mask-included pixel, the mean absolute height difference to all
    other mask-included pixels within *radius* feet is computed.  Pixels whose
    mean delta exceeds ``threshold_sigma`` × the standard deviation of those
    per-pixel means are replaced by the median of their masked neighbours
    (excluding the pixel itself).

    Args:
        heightmap: ``(W, H)`` uint16 raster — heights in inches.
        mask: ``(W, H)`` uint8 raster — 255 inside footprint, 0 outside.
        radius: Neighbourhood radius in feet (= pixels, 1 usft/pixel).
        threshold_sigma: Multiplier on the global std dev used as the outlier
            threshold.

    Returns:
        A new ``uint16`` array of the same shape with outliers corrected.
        Pixels outside the mask are unchanged.
    """
    offsets = _circular_offsets(radius)
    if not offsets:
        return heightmap.copy()

    W, H = heightmap.shape
    mask_bool = mask == 255
    heights_f = heightmap.astype(np.float64)

    # --- Vectorised mean-delta computation ---
    # Pad both arrays so that out-of-bounds neighbours are treated as absent.
    pad = int(np.ceil(radius))
    heights_pad = np.pad(heights_f, pad, mode="constant", constant_values=0.0)
    mask_pad = np.pad(mask_bool.astype(np.float64), pad, mode="constant", constant_values=0.0)

    sum_delta = np.zeros((W, H), dtype=np.float64)
    count = np.zeros((W, H), dtype=np.float64)

    for di, dj in offsets:
        # Slice the padded arrays to get neighbour values aligned to the output grid.
        r0, r1 = pad + di, pad + di + W
        c0, c1 = pad + dj, pad + dj + H
        neighbour_h = heights_pad[r0:r1, c0:c1]
        neighbour_m = mask_pad[r0:r1, c0:c1]
        sum_delta += np.abs(heights_f - neighbour_h) * neighbour_m
        count += neighbour_m

    safe_count = np.where(count > 0, count, 1.0)
    mean_delta = np.where(count > 0, sum_delta / safe_count, 0.0)

    # Global statistics over mask pixels only.
    mask_vals = mean_delta[mask_bool]
    if mask_vals.size == 0:
        return heightmap.copy()

    global_std = mask_vals.std()
    if global_std == 0.0:
        return heightmap.copy()

    threshold = threshold_sigma * global_std

    # --- Replace outlier pixels with neighbourhood median ---
    result = heightmap.copy()
    outlier_pixels = np.argwhere(mask_bool & (mean_delta > threshold))

    for xi, yi in outlier_pixels:
        neighbours = []
        for di, dj in offsets:
            ni, nj = xi + di, yi + dj
            if 0 <= ni < W and 0 <= nj < H and mask_bool[ni, nj]:
                neighbours.append(heights_f[ni, nj])
        if neighbours:
            result[xi, yi] = int(round(np.median(neighbours)))

    return result
