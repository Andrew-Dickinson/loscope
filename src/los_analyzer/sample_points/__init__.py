"""3D sample point generation from a building heightmap."""
from __future__ import annotations

import numpy as np
import shapely

from .combine import cull_and_combine
from .grid import sample_grid
from .mast import apply_mast_offset
from .perimeter import sample_perimeter

__all__ = [
    "generate_sample_points",
    "apply_mast_offset",
    "sample_grid",
    "sample_perimeter",
    "cull_and_combine",
]


def generate_sample_points(
    heightmap: np.ndarray,
    x_sw: int,
    y_sw: int,
    spacing: int,
    mask: np.ndarray | None = None,
    polygon: shapely.Geometry | None = None,
) -> np.ndarray:
    """Generate a grid of 3D sample points from a building heightmap.

    XY grid points are centred in each *spacing*×*spacing* cell; Z values come
    from the heightmap.  Cliff compensation adds extra Z columns where
    neighbouring grid samples differ by more than *spacing* feet vertically.

    When *polygon* is supplied, additional points are sampled along every ring
    (exterior + holes) at *spacing*-foot intervals, and any base grid point
    whose XY lies within *spacing* feet of a perimeter point is culled.  Cliff
    points are never culled.

    Parameters
    ----------
    heightmap:
        ``(W, H)`` uint16 array, heights in inches.  Origin at *(x_sw, y_sw)*.
    x_sw, y_sw:
        SW corner in EPSG:6539.
    spacing:
        Grid cell size in feet (integer, >= 1).
    mask:
        Optional ``(W, H)`` uint8; non-zero = inside the region of interest.
    polygon:
        Optional shapely Polygon / MultiPolygon for perimeter sampling.

    Returns
    -------
    ``(N, 3)`` float64 ``[easting, northing, z_feet]`` in EPSG:6539.
    """
    base_pts, cliff_pts = sample_grid(heightmap, x_sw, y_sw, spacing, mask=mask)

    if polygon is None:
        parts = [p for p in (base_pts, cliff_pts) if len(p)]
        return np.vstack(parts) if parts else np.empty((0, 3), dtype=np.float64)

    perim_pts = sample_perimeter(polygon, heightmap, x_sw, y_sw, spacing)
    return cull_and_combine(base_pts, cliff_pts, perim_pts, spacing / 2.0)
