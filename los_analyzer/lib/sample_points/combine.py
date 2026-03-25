"""Cull grid base points near perimeter points and assemble final set."""
from __future__ import annotations

import numpy as np
from scipy.spatial import cKDTree


def cull_and_combine(
    base_pts: np.ndarray,
    cliff_pts: np.ndarray,
    perim_pts: np.ndarray,
    cull_radius: float,
) -> np.ndarray:
    """Combine grid and perimeter points, culling nearby grid base points.

    Any base grid point whose XY lies within *cull_radius* feet (Euclidean) of
    any perimeter point is removed

    Parameters
    ----------
    base_pts:
        ``(M, 3)`` float64 base grid points.
    cliff_pts:
        ``(K, 3)`` float64 cliff compensation points.
    perim_pts:
        ``(P, 3)`` float64 perimeter points.
    cull_radius:
        Cull threshold in feet (same unit as coordinates).

    Returns
    -------
    ``(N, 3)`` float64: perimeter + surviving base + all cliff points.
    """
    surviving: list[np.ndarray] = []

    parts: list[np.ndarray] = []
    if len(base_pts):
        parts.append(base_pts)

    if len(cliff_pts):
        parts.append(cliff_pts)

    for part in parts:
        if len(perim_pts):
            perim_tree = cKDTree(perim_pts)
            dist, _ = perim_tree.query(part)
            surviving.append(part[dist >= cull_radius])
        else:
            surviving.append(part)

    if len(perim_pts):
        surviving.append(perim_pts)

    return np.vstack(surviving) if surviving else np.empty((0, 3), dtype=np.float64)
