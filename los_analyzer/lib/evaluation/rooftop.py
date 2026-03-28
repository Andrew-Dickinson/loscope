"""Per-point Fresnel zone obstruction evaluation for rooftop sample points.

Given a set of measurement positions (e.g. from a building rooftop) and a
common far-end point (e.g. a tower), evaluates whether each position has a
clear line of sight using Fresnel zone intersection analysis.
"""
from __future__ import annotations

import dataclasses
from enum import Enum
from pathlib import Path
from typing import List

import numpy as np
from tqdm import tqdm

from los_analyzer.lib.fresnel.fresnel_zone2 import compute_fresnel_zone, FresnelZone
from los_analyzer.lib.tiles.identify import identify_tiles
from los_analyzer.lib.tiles.intersect import compute_intersection, IntersectionGrid
from los_analyzer.lib.tiles.load import load_terrain_grid


class ObstructionStatus(str, Enum):
    UNOBSTRUCTED = "unobstructed"
    PARTIALLY_OBSTRUCTED = "partially_obstructed"  # alpha=1.0 blocked, alpha=0.6 clear
    OBSTRUCTED = "obstructed"  # alpha=0.6 blocked


@dataclasses.dataclass
class SamplePointEvaluation:
    point_a_nys: tuple[float, float, float]  # (3,) float64 [easting, northing, z_feet]
    point_b_nys: tuple[float, float, float]  # (3,) float64 [easting, northing, z_feet]
    status: ObstructionStatus
    max_obstruction_full: float  # max obstruction value in zone with alpha=1.0
    max_obstruction_partial: float  # max obstruction value in zone with alpha=0.6
    tile_ids: List[str]
    frequency_hz: float
    zone_full: FresnelZone
    zone_partial: FresnelZone
    intersection_full: IntersectionGrid
    intersection_partial: IntersectionGrid


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

    for pt in tqdm(measurement_pts, desc="Evaluating sample points", unit="pt"):
        p_nys = (float(pt[0]), float(pt[1]), float(pt[2]))
        results.append(
            evaluate_point(p_nys, common_pt_nys, frequency_hz, tile_dir, obstruction_dir, obstruction_types)
        )

    return results


def evaluate_point(
    pt_a_nys: tuple[float, float, float],
    pt_b_nys: tuple[float, float, float],
    frequency_hz: float,
    tile_dir: Path,
    obstruction_dir: Path | None = None,
    obstruction_types: str | list[str] = "*",
) -> SamplePointEvaluation:
    zone_full = compute_fresnel_zone(pt_a_nys, pt_b_nys, frequency_hz, alpha=1.0)
    zone_partial = compute_fresnel_zone(pt_a_nys, pt_b_nys, frequency_hz, alpha=0.6)

    # TODO: Fix: missing tiles don't get fetched here
    tiles = identify_tiles(zone_full, tile_dir, require_exists=True)

    terrain_full = load_terrain_grid(
        zone_full, tiles, tile_dir, obstruction_types, obstruction_dir
    )
    terrain_partial = load_terrain_grid(
        zone_partial, tiles, tile_dir, obstruction_types, obstruction_dir
    )

    intersection_full = compute_intersection(zone_full, terrain_full)
    intersection_partial = compute_intersection(zone_partial, terrain_partial)

    max_full = _valid_max(intersection_full)
    max_partial = _valid_max(intersection_partial)

    if max_full == 0.0:
        status = ObstructionStatus.UNOBSTRUCTED
    elif max_partial == 0.0:
        status = ObstructionStatus.PARTIALLY_OBSTRUCTED
    else:
        status = ObstructionStatus.OBSTRUCTED

    return  SamplePointEvaluation(
        point_a_nys=pt_a_nys,
        point_b_nys=pt_b_nys,
        status=status,
        max_obstruction_full=max_full,
        max_obstruction_partial=max_partial,
        tile_ids=tiles,
        zone_full=zone_full,
        zone_partial=zone_partial,
        frequency_hz=frequency_hz,
        intersection_full=intersection_full,
        intersection_partial=intersection_partial,
    )