"""Perimeter point sampling along polygon rings."""
from __future__ import annotations

from typing import Iterator

import numpy as np
import shapely
from shapely.geometry.base import BaseGeometry


def sample_perimeter(
    polygon: BaseGeometry,
    heightmap: np.ndarray,
    x_sw: int,
    y_sw: int,
    spacing: int,
    mask: np.ndarray | None = None,
) -> np.ndarray:
    """Sample points at *spacing*-foot intervals along all rings of *polygon*.

    Covers both the exterior ring and any interior rings (holes) of a Polygon
    or MultiPolygon.  Z is looked up from *heightmap* at each point's pixel.

    When *mask* is supplied, points whose floored pixel lands outside the mask
    are relocated to the nearest valid neighbour (±1 pixel).  Points with no
    valid neighbour are skipped.

    Returns ``(N, 3)`` float64 ``[easting, northing, z_feet]``.
    """
    W, H = heightmap.shape
    pts: list[tuple[float, float, float]] = []
    step = float(spacing)

    for ring in _iter_rings(polygon):
        length = ring.length
        if length == 0.0:
            continue
        for d in np.arange(0.0, length, step):
            pt = ring.interpolate(d)
            x, y = pt.x, pt.y
            ei = int(np.clip(int(np.floor(x - x_sw)), 0, W - 1))
            ni = int(np.clip(int(np.floor(y - y_sw)), 0, H - 1))
            if mask is not None and mask[ei, ni] == 0:
                # Search ±1 pixel neighbourhood for a valid masked pixel.
                found = False
                for dei in (-1, 0, 1):
                    for dni in (-1, 0, 1):
                        if dei == 0 and dni == 0:
                            continue
                        nei2 = ei + dei
                        nni2 = ni + dni
                        if 0 <= nei2 < W and 0 <= nni2 < H and mask[nei2, nni2] != 0:
                            ei, ni = nei2, nni2
                            found = True
                            break
                    if found:
                        break
                if not found:
                    continue
            z = float(heightmap[ei, ni]) / 12.0
            pts.append((x, y, z))

    return np.array(pts, dtype=np.float64) if pts else np.empty((0, 3), dtype=np.float64)


def _iter_rings(polygon: BaseGeometry) -> Iterator:
    """Yield all rings (exterior + holes) from a Polygon or MultiPolygon."""
    geoms = polygon.geoms if hasattr(polygon, "geoms") else (polygon,)
    for geom in geoms:
        yield geom.exterior
        yield from geom.interiors
