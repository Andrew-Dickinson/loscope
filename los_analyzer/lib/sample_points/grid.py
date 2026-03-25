"""Grid-based 3D sample point generation with cliff compensation."""
from __future__ import annotations

import numpy as np


def sample_grid(
    heightmap: np.ndarray,
    x_sw: int,
    y_sw: int,
    spacing: int,
    mask: np.ndarray | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """Sample points on an XY grid centred within each spacing×spacing cell.

    Parameters
    ----------
    heightmap:
        ``(W, H)`` uint16 array, heights in inches.
    x_sw, y_sw:
        SW corner of the heightmap in EPSG:6539.
    spacing:
        Grid cell size in feet (integer, >= 1).
    mask:
        Optional ``(W, H)`` uint8; non-zero = inside.  Only inside pixels are
        sampled.

    Returns
    -------
    base_pts : ``(M, 3)`` float64 — one point per XY sample inside the mask.
    cliff_pts : ``(K, 3)`` float64 — extra Z points for vertical cliff faces;
        each shares its ``(easting, northing)`` with a *base_pts* entry.
    """
    if spacing < 1:
        raise ValueError("spacing must be >= 1")

    W, H = heightmap.shape
    trigger_in = spacing * 12        # cliff detection threshold, inches (one full grid spacing)
    cliff_step_in = spacing * 6      # vertical step between cliff points, inches (half grid spacing)

    ei_arr = np.arange(spacing // 2, W, spacing)
    ni_arr = np.arange(spacing // 2, H, spacing)
    _empty = np.empty((0, 3), dtype=np.float64)
    if ei_arr.size == 0 or ni_arr.size == 0:
        return _empty, _empty

    ei_grid, ni_grid = np.meshgrid(ei_arr, ni_arr, indexing="ij")  # (nW, nH)
    h_grid = heightmap[ei_grid, ni_grid].astype(np.float64)         # inches

    x_grid = (x_sw + ei_grid + 0.5).astype(np.float64)
    y_grid = (y_sw + ni_grid + 0.5).astype(np.float64)

    inside = (
        mask[ei_grid, ni_grid].astype(bool)
        if mask is not None
        else np.ones(ei_grid.shape, dtype=bool)
    )

    keep = inside.ravel()
    base_pts = np.stack(
        [x_grid.ravel()[keep], y_grid.ravel()[keep], h_grid.ravel()[keep] / 12.0],
        axis=1,
    )

    # ── Cliff compensation ────────────────────────────────────────────────────
    # For each inside sample pixel find the tallest 4-connected neighbour on
    # the sample grid (pixel offset ±spacing in one axis).
    nW, nH = ei_grid.shape
    max_nbr_h = np.full((nW, nH), -np.inf, dtype=np.float64)

    for dei, dni in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        nbr_ei = ei_grid + dei * spacing
        nbr_ni = ni_grid + dni * spacing
        valid = (nbr_ei >= 0) & (nbr_ei < W) & (nbr_ni >= 0) & (nbr_ni < H)
        nbr_h = np.where(
            valid,
            heightmap[
                np.clip(nbr_ei, 0, W - 1),
                np.clip(nbr_ni, 0, H - 1),
            ].astype(np.float64),
            -np.inf,
        )
        max_nbr_h = np.maximum(max_nbr_h, nbr_h)

    cliff_ij = np.argwhere((max_nbr_h - h_grid > trigger_in) & inside)

    extra_list: list[np.ndarray] = []
    for i, j in cliff_ij:
        h0 = h_grid[i, j]
        h_top = max_nbr_h[i, j]
        n_extra = int((h_top - h0) / cliff_step_in)
        if n_extra < 1:
            continue
        z_ft = (h0 + np.arange(1, n_extra + 2) * cliff_step_in) / 12.0  # +1 cap step
        x = x_grid[i, j]
        y = y_grid[i, j]
        n = len(z_ft)
        extra_list.append(
            np.column_stack([np.full(n, x), np.full(n, y), z_ft])
        )

    cliff_pts = np.vstack(extra_list) if extra_list else _empty
    return base_pts, cliff_pts
