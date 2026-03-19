"""Per-point Fresnel zone obstruction evaluation for rooftop sample points.

Given a set of measurement positions (e.g. from a building rooftop) and a
common far-end point (e.g. a tower), evaluates whether each position has a
clear line of sight using Fresnel zone intersection analysis.
"""
from __future__ import annotations

import dataclasses
from enum import Enum
from pathlib import Path

import numpy as np

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone
from los_analyzer.tiles.identify import identify_tiles
from los_analyzer.tiles.intersect import compute_intersection
from los_analyzer.tiles.load import load_terrain_grid


class ObstructionStatus(str, Enum):
    UNOBSTRUCTED = "unobstructed"
    PARTIALLY_OBSTRUCTED = "partially_obstructed"  # alpha=1.0 blocked, alpha=0.6 clear
    FULLY_OBSTRUCTED = "fully_obstructed"  # alpha=0.6 blocked


@dataclasses.dataclass
class SamplePointEvaluation:
    point: np.ndarray  # (3,) float64 [easting, northing, z_feet]
    status: ObstructionStatus
    max_obstruction_alpha1: float  # max obstruction value in zone with alpha=1.0
    max_obstruction_alpha06: float  # max obstruction value in zone with alpha=0.6


def _valid_max(obs) -> float:
    """Return the maximum obstruction value over valid (within-width) cells.

    Uses the widths mask to ignore padding beyond each row's valid entries.
    Returns 0.0 if there are no valid cells.
    """
    valid_mask = np.arange(obs.values.shape[1])[None, :] < obs.widths[:, None]
    if not valid_mask.any():
        return 0.0
    return float(obs.values[valid_mask].max())


def evaluate_sample_points(
    measurement_pts: np.ndarray,
    common_pt_nys: tuple[float, float, float],
    frequency_hz: float,
    tile_dir: Path,
    obstruction_dir: Path | None = None,
    obstruction_types: str | list[str] = "*",
) -> list[SamplePointEvaluation]:
    """Evaluate line-of-sight status for each measurement point.

    For each point in *measurement_pts*, computes Fresnel zones at two radii
    (alpha=1.0 and alpha=0.6) towards *common_pt_nys*, loads the terrain in
    those zones, and determines the obstruction level.

    Args:
        measurement_pts: ``(N, 3)`` float64 array of positions in NYS
            EPSG:6539 coordinates ``[easting, northing, z_feet]``.
        common_pt_nys: The far-end point ``(easting, northing, z_feet)`` in
            NYS EPSG:6539.
        frequency_hz: Link frequency in Hz (e.g. ``24e9`` for 24 GHz).
        tile_dir: Directory containing preprocessed LiDAR tile .tif files.
        obstruction_dir: Directory containing obstruction tif+json pairs.
            If None, obstructions are not loaded.
        obstruction_types: Which obstruction types to include — ``"*"`` for
            all, or a list of type strings.

    Returns:
        A list of :class:`SamplePointEvaluation` objects, one per input point,
        in the same order as *measurement_pts*.
    """
    tile_dir = Path(tile_dir)
    results: list[SamplePointEvaluation] = []

    for pt in measurement_pts:
        p_nys = (float(pt[0]), float(pt[1]), float(pt[2]))

        zone_1 = compute_fresnel_zone(p_nys, common_pt_nys, frequency_hz, alpha=1.0)
        zone_06 = compute_fresnel_zone(p_nys, common_pt_nys, frequency_hz, alpha=0.6)

        tiles_1 = identify_tiles(zone_1, tile_dir, require_exists=True)
        tiles_06 = identify_tiles(zone_06, tile_dir, require_exists=True)

        terrain_1 = load_terrain_grid(
            zone_1, tiles_1, tile_dir, obstruction_types, obstruction_dir
        )
        terrain_06 = load_terrain_grid(
            zone_06, tiles_06, tile_dir, obstruction_types, obstruction_dir
        )

        obs_1 = compute_intersection(zone_1, terrain_1)
        obs_06 = compute_intersection(zone_06, terrain_06)

        max_1 = _valid_max(obs_1)
        max_06 = _valid_max(obs_06)

        if max_1 == 0.0:
            status = ObstructionStatus.UNOBSTRUCTED
        elif max_06 == 0.0:
            status = ObstructionStatus.PARTIALLY_OBSTRUCTED
        else:
            status = ObstructionStatus.FULLY_OBSTRUCTED

        results.append(
            SamplePointEvaluation(
                point=np.array(pt, dtype=np.float64),
                status=status,
                max_obstruction_alpha1=max_1,
                max_obstruction_alpha06=max_06,
            )
        )

    return results
