"""3D sample point generation from a building heightmap."""
from __future__ import annotations

from cachetools import cached, LRUCache
from cachetools.keys import hashkey
from typing import NamedTuple, List

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

from ..building.heightmap import RooftopHeightMap


class EncodedPoint(NamedTuple):
    x: float
    y: float
    z: float
    nys_e: float
    nys_n: float
    nys_z: float

def point_encode(pt: np.ndarray, x_sw: float, y_sw: float) -> EncodedPoint:
    return EncodedPoint(
        x=round(float(pt[0]) - x_sw, 3),
        y=round(float(pt[1]) - y_sw, 3),
        z=round(float(pt[2]), 3),
        nys_e=round(float(pt[0]), 3),
        nys_n=round(float(pt[1]), 3),
        nys_z=round(float(pt[2]), 3),
    )

class SamplePoint(NamedTuple):
    displayPoint: EncodedPoint
    measurementPoint: EncodedPoint

@cached(
    cache=LRUCache(128),
    key=lambda model, sample_spacing, mast_offset: hashkey(model.bin_id, sample_spacing, mast_offset)
)
def get_paired_sample_points(model: RooftopHeightMap, sample_spacing: int, mast_offset: float) -> List[SamplePoint]:
    """
    Given a hieghtmap representing a rooftop, generate points which are roughly evenly spaced over the rooftop based
    on sample_spacing, with extra points at areas of large hieght change and around the perimeter. For each sample
    point, we provide a "display" location as well as a "measurement" location which is usually offset upwards
    by mast_offset
    """
    raw_pts = generate_sample_points(
        model.heightmap, model.x_sw,  model.y_sw, sample_spacing,
        mask=model.mask, polygon=model.poly_nys,
    )
    display_pts, measurement_pts = apply_mast_offset(raw_pts, mast_offset)
    return  [
        SamplePoint(
            displayPoint=point_encode(dp, model.x_sw, model.y_sw),
            measurementPoint=point_encode(mp, model.x_sw, model.y_sw),
        )
        for dp, mp in zip(display_pts, measurement_pts)
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

    perim_pts = sample_perimeter(polygon, heightmap, x_sw, y_sw, spacing, mask=mask)
    return cull_and_combine(base_pts, cliff_pts, perim_pts, spacing / 2.0)
