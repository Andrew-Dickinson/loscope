from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.obstructions.io import load_obstruction, obstructions_for_tile_id
from los_analyzer.lib.preprocessing.io import load_tile


@dataclass
class TerrainGrid:
    heights: np.ndarray           # uint16, shape (H, maxW); row i has widths[i] valid entries
    widths: np.ndarray            # uint32, shape (H,); matches FresnelZone exactly
    offsets: np.ndarray           # uint32, shape (H,); matches FresnelZone exactly
    x_base_offset: int
    y_base_offset: int
    matched_obstruction_ids: list[str] = field(default_factory=list)


def load_terrain_grid(
    fresnel_zone: FresnelZone,
    tile_ids: list[str],
    tile_dir: Path,
    obstruction_types: list[str] | str,
    obstruction_dir:  Path,
) -> TerrainGrid:
    """Build a TerrainGrid aligned to fresnel_zone by loading tiles and additional obstructions.

    When obstruction_types='*' all additional obstruction types are included; otherwise only
    those whose type string is in the list are included.  obstruction_dir is the directory
    containing obstruction tif+json pairs; if None, obstruction loading is skipped.
    """
    H = int(fresnel_zone.widths.shape[0])
    max_w = fresnel_zone.top.shape[1] if H > 0 else 1
    heights = np.zeros((H, max_w), dtype=np.uint16)

    obstruction_ids_by_type = defaultdict(list)
    for tile_id in tile_ids:
        tile = load_tile(tile_id, tile_dir)
        _blit_tile(tile, fresnel_zone, heights)

        obstructions_for_tile = obstructions_for_tile_id(tile_id, obstruction_dir)
        for obs_type, obs_ids in obstructions_for_tile.items():
            obstruction_ids_by_type[obs_type].extend(obs_ids)

    allowed_types = set(obstruction_types) if obstruction_types != '*' else set(obstruction_ids_by_type.keys())
    matched_ids = []
    for obs_type, obs_ids in obstruction_ids_by_type.items():
        if obs_type in allowed_types:
            matched_ids.extend(obs_ids)

    for obs_id in matched_ids:
        _apply_obstruction(obs_id, fresnel_zone, heights, obstruction_dir)

    return TerrainGrid(
        heights=heights,
        widths=fresnel_zone.widths.copy(),
        offsets=fresnel_zone.offsets.copy(),
        x_base_offset=fresnel_zone.x_base_offset,
        y_base_offset=fresnel_zone.y_base_offset,
        matched_obstruction_ids=matched_ids,
    )


def _blit_tile(tile, fresnel_zone: FresnelZone, heights: np.ndarray) -> None:
    """Copy tile raster heights into heights wherever the tile overlaps the fresnel zone grid."""
    x_off = tile.x_offset
    y_off = tile.y_offset
    tile_w, tile_h = tile.raster.shape  # (500, 500): axes [easting_local, northing_local]

    H = heights.shape[0]
    y_base = fresnel_zone.y_base_offset
    x_base = fresnel_zone.x_base_offset

    i_start = max(0, y_off - y_base)
    i_end = min(H, y_off + tile_h - y_base)

    for i in range(i_start, i_end):
        width = int(fresnel_zone.widths[i])
        if width == 0:
            continue
        dy = (y_base + i) - y_off

        row_e_start = x_base + int(fresnel_zone.offsets[i])
        row_e_end = row_e_start + width

        overlap_e_start = max(row_e_start, x_off)
        overlap_e_end = min(row_e_end, x_off + tile_w)
        if overlap_e_start >= overlap_e_end:
            continue

        j_start = overlap_e_start - row_e_start
        j_end = overlap_e_end - row_e_start
        dx_start = overlap_e_start - x_off
        dx_end = overlap_e_end - x_off

        heights[i, j_start:j_end] = tile.raster[dx_start:dx_end, dy]


def _apply_obstruction(
    obs_id: str,
    fresnel_zone: FresnelZone,
    heights: np.ndarray,
    obs_dir: Path,
) -> None:
    """Apply an additional obstruction to heights using element-wise max."""
    obs = load_obstruction(obs_id, obs_dir)
    x_off = obs.x_offset
    y_off = obs.y_offset
    obs_w, obs_h = obs.raster.shape  # (W, H): axes [easting_local, northing_local]

    H = heights.shape[0]
    y_base = fresnel_zone.y_base_offset
    x_base = fresnel_zone.x_base_offset

    i_start = max(0, y_off - y_base)
    i_end = min(H, y_off + obs_h - y_base)

    for i in range(i_start, i_end):
        width = int(fresnel_zone.widths[i])
        if width == 0:
            continue
        dy = (y_base + i) - y_off

        row_e_start = x_base + int(fresnel_zone.offsets[i])
        row_e_end = row_e_start + width

        overlap_e_start = max(row_e_start, x_off)
        overlap_e_end = min(row_e_end, x_off + obs_w)
        if overlap_e_start >= overlap_e_end:
            continue

        j_start = overlap_e_start - row_e_start
        j_end = overlap_e_end - row_e_start
        dx_start = overlap_e_start - x_off
        dx_end = overlap_e_end - x_off

        obs_row = obs.raster[dx_start:dx_end, dy]
        heights[i, j_start:j_end] = np.maximum(heights[i, j_start:j_end], obs_row)
