from __future__ import annotations

import math
from typing import Optional

import PIL
import numpy as np
from PIL import Image

from los_analyzer.lib.coordinates.coordinate_translate import translate_from_nys_plane
from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.lib.tiles.intersect import IntersectionGrid
from los_analyzer.lib.tiles.rasterize import rasterize_stairstep_grid_for_tile

_C_USFT_PER_S = 299_792_458 / 0.3048006096

def fresnel_ellipse_ring(
    nys_a: tuple,
    nys_b: tuple,
    frequency_hz: float,
    alpha: float = 1.0,
    n_pts: int = 90,
) -> list[list[float]]:
    cx = (nys_a[0] + nys_b[0]) / 2
    cy = (nys_a[1] + nys_b[1]) / 2
    dx = nys_b[0] - nys_a[0]
    dy = nys_b[1] - nys_a[1]
    L = math.sqrt(dx**2 + dy**2)
    if L == 0:
        return []
    theta = math.atan2(dy, dx)
    semi_major = L / 2
    wavelength_usft = _C_USFT_PER_S / frequency_hz
    semi_minor = alpha * math.sqrt(wavelength_usft * L / 4)
    ring = []
    for i in range(n_pts + 1):
        t = 2 * math.pi * i / n_pts
        xl = semi_major * math.cos(t)
        yl = semi_minor * math.sin(t)
        e = cx + xl * math.cos(theta) - yl * math.sin(theta)
        n = cy + xl * math.sin(theta) + yl * math.cos(theta)
        ring.append(translate_from_nys_plane((e, n)))
    return ring


def intersection_image_for_tile(
    tile_id: str,
    intersection_grid: IntersectionGrid,
) -> Optional[PIL.Image]:
    rasterized_intersection = rasterize_intersection_grid_for_tile(tile_id, intersection_grid)
    if rasterized_intersection.max() == 0:
        return None

    # SunsetDark-inspired: saturated yellow → orange → red → very dark red (near-purple)
    # Control points at t = 0.0, 0.25, 0.5, 0.75, 1.0
    _stops = np.array([0.0, 0.4, 0.6, 0.75, 1.0])
    _r     = np.array([255, 255,  210, 138,  214], dtype=np.float32)
    _g     = np.array([215, 105,   18,   0,    2], dtype=np.float32)
    _b     = np.array([  0,   0,   28,  16,   52], dtype=np.float32)

    v = rasterized_intersection.ravel()
    rgba = np.zeros((TILE_SIDE_USFT * TILE_SIDE_USFT, 4), dtype=np.uint8)
    rgba[:, 0] = np.interp(v, _stops, _r).astype(np.uint8)
    rgba[:, 1] = np.interp(v, _stops, _g).astype(np.uint8)
    rgba[:, 2] = np.interp(v, _stops, _b).astype(np.uint8)
    rgba[:, 3] = np.where(v > 0, 255, 0).astype(np.uint8)
    rgba = rgba.reshape((TILE_SIDE_USFT, TILE_SIDE_USFT, 4))

    rgba = rgba[::-1]
    rgba = np.repeat(np.repeat(rgba, 8, axis=0), 8, axis=1)

    # 2-px black outline: any transparent pixel within 2px of an opaque pixel
    filled = rgba[:, :, 3] > 0
    dilated = filled
    for _ in range(2):
        dilated = (
            np.roll(dilated,  1, axis=0) |
            np.roll(dilated, -1, axis=0) |
            np.roll(dilated,  1, axis=1) |
            np.roll(dilated, -1, axis=1) |
            dilated
        )
    border = dilated & ~filled
    rgba[border] = (0, 0, 0, 255)

    return Image.fromarray(rgba, "RGBA")



def rasterize_intersection_grid_for_tile(
    tile_id: str,
    intersection_grid: IntersectionGrid,
) -> np.ndarray:
    return rasterize_stairstep_grid_for_tile(
        tile_id,
        intersection_grid.widths,
        intersection_grid.offsets,
        intersection_grid.values,
        (intersection_grid.x_base_offset, intersection_grid.y_base_offset)
    )
