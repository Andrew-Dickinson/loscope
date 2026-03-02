from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from los_analyzer.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.preprocessing.io import load_tile


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
    tile_dir: str | Path,
    obstruction_types: list[str] | str,
) -> TerrainGrid:
    """Build a TerrainGrid aligned to fresnel_zone by loading tiles and additional obstructions.

    When obstruction_types='*' all additional obstruction types are included; otherwise only
    those whose type string is in the list are included.  Because additional-obstruction
    loading is not yet implemented, matched_obstruction_ids is always empty.
    """
    tile_dir = Path(tile_dir)
    H = int(fresnel_zone.widths.shape[0])
    max_w = int(fresnel_zone.widths.max()) if H > 0 else 0
    heights = np.zeros((H, max(max_w, 1)), dtype=np.uint16)

    seen_obstruction_ids: set[str] = set()

    for tile_id in tile_ids:
        tile = load_tile(tile_id, tile_dir)
        _blit_tile(tile, fresnel_zone, heights)
        seen_obstruction_ids.update(tile.obstruction_ids)

    matched_ids = _filter_obstruction_ids(seen_obstruction_ids, obstruction_types)

    for obs_id in matched_ids:
        _apply_obstruction(obs_id, fresnel_zone, heights, tile_dir)

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


def _filter_obstruction_ids(
    obstruction_ids: set[str],
    obstruction_types: list[str] | str,
) -> list[str]:
    """Return sorted obstruction IDs matching the type filter.

    Type-based filtering requires obstruction metadata that is not yet implemented;
    consequently this returns all collected IDs regardless of type.  In practice,
    tile.obstruction_ids is always empty so the result is always [].
    """
    if not obstruction_ids:
        return []
    return sorted(obstruction_ids)


def _apply_obstruction(
    obs_id: str,
    fresnel_zone: FresnelZone,
    heights: np.ndarray,
    tile_dir: Path,
) -> None:
    """Apply an additional obstruction to heights using element-wise max.

    Not yet implemented — additional obstructions are not defined in Part 1.
    """
    raise NotImplementedError(f"Additional obstruction loading not yet implemented: {obs_id}")
