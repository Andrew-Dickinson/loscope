from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

import numpy as np

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.tiles.load import TerrainGrid

# XY radius (in US survey feet) around each Fresnel zone endpoint within which
# obstruction values are zeroed out in the intersection grid.
ENDPOINT_EXCLUSION_RADIUS_USFT: float = 4.0


@dataclass
class IntersectionGrid:
    values: np.ndarray    # float32, shape (H, maxW); row i has widths[i] valid entries
    widths: np.ndarray    # uint32, shape (H,); copied from FresnelZone
    offsets: np.ndarray   # uint32, shape (H,); copied from FresnelZone
    x_base_offset: int
    y_base_offset: int


def compute_intersection(
    fresnel_zone: FresnelZone,
    terrain: TerrainGrid,
    pt_a_nys: Optional[tuple[float, float, float]] = None,
    pt_b_nys: Optional[tuple[float, float, float]] = None,
) -> IntersectionGrid:
    """Compute the per-cell obstruction level from a FresnelZone and a TerrainGrid.

    For each valid cell: (terrain - bottom) / (top - bottom), clipped to [0, 1].
    When top == bottom (zero-height fresnel zone), the cell contributes 0.
    Cells beyond widths[i] in each row are set to 0.
    """
    top = fresnel_zone.top.astype(np.float32)
    bottom = fresnel_zone.bottom.astype(np.float32)
    terrain_h = terrain.heights.astype(np.float32)

    span = top - bottom
    # Avoid division by zero: where span is 0 the numerator is also 0 (terrain==bottom==top),
    # so the pre-clip result is 0/1 = 0 regardless.
    safe_span = np.where(span == 0.0, 1.0, span)

    values = np.clip((terrain_h - bottom) / safe_span, 0.0, 1.0).astype(np.float32)
    values[top <= 0] = 1 # If the top is below the ground, we count this as 100% intersection

    # Zero out entries beyond each row's valid width
    H, maxW = values.shape
    widths = fresnel_zone.widths
    for i in range(H):
        w = int(widths[i])
        if w < maxW:
            values[i, w:] = 0.0

    # Zero out cells within the exclusion radius of either endpoint
    if pt_a_nys is not None or pt_b_nys is not None:
        j = np.arange(maxW)[None, :]                                          # (1, maxW)
        x = fresnel_zone.x_base_offset + fresnel_zone.offsets[:, None] + j   # (H, maxW)
        y = (fresnel_zone.y_base_offset + np.arange(H))[:, None]             # (H, 1)
        r2 = ENDPOINT_EXCLUSION_RADIUS_USFT ** 2
        exclude = np.zeros((H, maxW), dtype=bool)
        if pt_a_nys is not None:
            exclude |= (x - pt_a_nys[0]) ** 2 + (y - pt_a_nys[1]) ** 2 <= r2
        if pt_b_nys is not None:
            exclude |= (x - pt_b_nys[0]) ** 2 + (y - pt_b_nys[1]) ** 2 <= r2
        values[exclude] = 0.0

    return IntersectionGrid(
        values=values,
        widths=fresnel_zone.widths.copy(),
        offsets=fresnel_zone.offsets.copy(),
        x_base_offset=fresnel_zone.x_base_offset,
        y_base_offset=fresnel_zone.y_base_offset,
    )
