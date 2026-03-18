"""Mast-offset computation: separate display and measurement positions."""
from __future__ import annotations

import numpy as np


def apply_mast_offset(
    pts: np.ndarray,
    offset: float,
) -> tuple[np.ndarray, np.ndarray]:
    """Split sample points into display and measurement positions.

    For every point that is the highest point at its (X, Y) location, the
    *measurement position* is shifted upward by *offset* feet.  All other
    points have coincident display and measurement positions.

    Parameters
    ----------
    pts:
        ``(N, 3)`` float64 array ``[easting, northing, z_feet]``.
    offset:
        Vertical mast offset in feet.

    Returns
    -------
    display_pts : ``(N, 3)`` float64 — original positions (unchanged copy).
    measurement_pts : ``(N, 3)`` float64 — top points shifted up by *offset*,
        all other points identical to *display_pts*.
    """
    display_pts = pts.copy()
    measurement_pts = pts.copy()

    if offset == 0.0 or len(pts) == 0:
        return display_pts, measurement_pts

    # Find the index of the highest point at each unique (X, Y).
    # XY values for grid/cliff points are exact floats (pixel centre + origin),
    # so dict-key comparison is safe.  Perimeter points are generally unique.
    top_index: dict[tuple[float, float], int] = {}
    for i in range(len(pts)):
        xy: tuple[float, float] = (pts[i, 0], pts[i, 1])
        if xy not in top_index or pts[i, 2] > pts[top_index[xy], 2]:
            top_index[xy] = i

    for i in top_index.values():
        measurement_pts[i, 2] += offset

    return display_pts, measurement_pts
