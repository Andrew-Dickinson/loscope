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
    any perimeter point is removed.  Cliff points — which share an XY with a
    base point — are **never** culled; they are always included in the output.

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
    parts: list[np.ndarray] = []

    if len(perim_pts):
        parts.append(perim_pts)

    if len(base_pts):
        if len(perim_pts):
            perim_tree = cKDTree(perim_pts[:, :2])
            dist, _ = perim_tree.query(base_pts[:, :2])
            surviving = base_pts[dist >= cull_radius]
        else:
            surviving = base_pts
        if len(surviving):
            parts.append(surviving)

    if len(cliff_pts):
        parts.append(cliff_pts)

    return np.vstack(parts) if parts else np.empty((0, 3), dtype=np.float64)
