"""Tests for src.los_analyzer.evaluation.rooftop"""
from __future__ import annotations

from unittest.mock import MagicMock, patch

import numpy as np
import pytest

from los_analyzer.evaluation.rooftop import (
    ObstructionStatus,
    SamplePointEvaluation,
    _valid_max,
    evaluate_sample_points,
)
from los_analyzer.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.tiles.intersect import ObstructionGrid
from los_analyzer.tiles.load import TerrainGrid


# ---------------------------------------------------------------------------
# Helpers: build minimal FresnelZone / TerrainGrid / ObstructionGrid
# ---------------------------------------------------------------------------

def _make_zone(top_vals, bottom_vals, widths=None, offsets=None, x_base=0, y_base=0):
    """Build a FresnelZone from flat height arrays (one row per entry)."""
    n = len(top_vals)
    maxW = 1
    top = np.array(top_vals, dtype=np.uint16).reshape(n, maxW)
    bottom = np.array(bottom_vals, dtype=np.uint16).reshape(n, maxW)
    w = np.ones(n, dtype=np.uint32) if widths is None else np.array(widths, dtype=np.uint32)
    o = np.zeros(n, dtype=np.uint32) if offsets is None else np.array(offsets, dtype=np.uint32)
    return FresnelZone(top=top, bottom=bottom, widths=w, offsets=o,
                       x_base_offset=x_base, y_base_offset=y_base)


def _make_terrain(heights, widths=None, offsets=None, x_base=0, y_base=0):
    """Build a TerrainGrid from a flat height list (one row per entry)."""
    n = len(heights)
    maxW = 1
    h = np.array(heights, dtype=np.uint16).reshape(n, maxW)
    w = np.ones(n, dtype=np.uint32) if widths is None else np.array(widths, dtype=np.uint32)
    o = np.zeros(n, dtype=np.uint32) if offsets is None else np.array(offsets, dtype=np.uint32)
    return TerrainGrid(heights=h, widths=w, offsets=o, x_base_offset=x_base, y_base_offset=y_base)


def _make_obs(values_flat, widths=None, offsets=None, x_base=0, y_base=0):
    """Build an ObstructionGrid with the given obstruction level values."""
    n = len(values_flat)
    maxW = 1
    v = np.array(values_flat, dtype=np.float32).reshape(n, maxW)
    w = np.ones(n, dtype=np.uint32) if widths is None else np.array(widths, dtype=np.uint32)
    o = np.zeros(n, dtype=np.uint32) if offsets is None else np.array(offsets, dtype=np.uint32)
    return ObstructionGrid(values=v, widths=w, offsets=o, x_base_offset=x_base, y_base_offset=y_base)


# ---------------------------------------------------------------------------
# _valid_max
# ---------------------------------------------------------------------------

def test_valid_max_returns_correct_max():
    """_valid_max should return the maximum valid obstruction value."""
    obs = _make_obs([0.2, 0.8, 0.5])
    assert _valid_max(obs) == pytest.approx(0.8)


def test_valid_max_empty_zone():
    """When all widths are 0, _valid_max should return 0.0."""
    n = 4
    v = np.zeros((n, 1), dtype=np.float32)
    w = np.zeros(n, dtype=np.uint32)  # all zero widths
    o = np.zeros(n, dtype=np.uint32)
    obs = ObstructionGrid(values=v, widths=w, offsets=o, x_base_offset=0, y_base_offset=0)
    assert _valid_max(obs) == pytest.approx(0.0)


def test_valid_max_respects_width_mask():
    """_valid_max should ignore padding columns beyond each row's valid width."""
    # Two rows, maxW=3; row 0 has width=1, row 1 has width=2
    v = np.array([[0.9, 0.9, 0.9],
                  [0.1, 0.3, 0.9]], dtype=np.float32)
    w = np.array([1, 2], dtype=np.uint32)
    o = np.zeros(2, dtype=np.uint32)
    obs = ObstructionGrid(values=v, widths=w, offsets=o, x_base_offset=0, y_base_offset=0)
    # valid cells: row0[0]=0.9, row1[0]=0.1, row1[1]=0.3  → max=0.9
    # but col 2 of row 1 (0.9) is beyond width=2 and should be ignored
    # Actually row0 width=1: only col0 valid; row1 width=2: cols 0,1 valid
    # → max over valid = max(0.9, 0.1, 0.3) = 0.9
    assert _valid_max(obs) == pytest.approx(0.9)


# ---------------------------------------------------------------------------
# ObstructionStatus enum values
# ---------------------------------------------------------------------------

def test_status_enum_values():
    """ObstructionStatus enum should have the expected string values."""
    assert ObstructionStatus.UNOBSTRUCTED.value == "unobstructed"
    assert ObstructionStatus.PARTIALLY_OBSTRUCTED.value == "partially_obstructed"
    assert ObstructionStatus.FULLY_OBSTRUCTED.value == "fully_obstructed"


def test_status_enum_is_str():
    """ObstructionStatus should be a str enum (values compare equal to plain strings)."""
    assert ObstructionStatus.UNOBSTRUCTED == "unobstructed"
    assert ObstructionStatus.FULLY_OBSTRUCTED == "fully_obstructed"


# ---------------------------------------------------------------------------
# evaluate_sample_points — mocked pipeline
#
# We patch the four pipeline calls inside evaluation.rooftop so tests run
# without real tile data.
# ---------------------------------------------------------------------------

_PATCH_FZ = "los_analyzer.evaluation.rooftop.compute_fresnel_zone"
_PATCH_IT = "los_analyzer.evaluation.rooftop.identify_tiles"
_PATCH_LT = "los_analyzer.evaluation.rooftop.load_terrain_grid"
_PATCH_CI = "los_analyzer.evaluation.rooftop.compute_intersection"


def _mock_pipeline(obs_1_vals, obs_06_vals):
    """Return (mock_fz, mock_it, mock_lt, mock_ci) for inject into patches.

    obs_1_vals / obs_06_vals: flat lists of obstruction values used for the
    alpha=1.0 and alpha=0.6 zones respectively.
    """
    mock_fz = MagicMock(return_value=_make_zone([1200], [600]))
    mock_it = MagicMock(return_value=[])
    mock_lt = MagicMock(return_value=_make_terrain([900]))

    call_count = [0]
    results = [_make_obs(obs_1_vals), _make_obs(obs_06_vals)]

    def _ci(zone, terrain):
        idx = call_count[0] % 2
        call_count[0] += 1
        return results[idx]

    mock_ci = MagicMock(side_effect=_ci)
    return mock_fz, mock_it, mock_lt, mock_ci


def test_unobstructed_when_terrain_below_zone(tmp_path):
    """When both alpha=1.0 and alpha=0.6 zones show no obstruction, status is UNOBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert len(results) == 1
    assert results[0].status == ObstructionStatus.UNOBSTRUCTED
    assert results[0].max_obstruction_alpha1 == pytest.approx(0.0)
    assert results[0].max_obstruction_alpha06 == pytest.approx(0.0)


def test_partially_obstructed_when_terrain_in_outer_ring(tmp_path):
    """When alpha=1.0 is blocked but alpha=0.6 is clear, status is PARTIALLY_OBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.5], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert results[0].status == ObstructionStatus.PARTIALLY_OBSTRUCTED
    assert results[0].max_obstruction_alpha1 == pytest.approx(0.5)
    assert results[0].max_obstruction_alpha06 == pytest.approx(0.0)


def test_fully_obstructed_when_terrain_above_alpha06(tmp_path):
    """When both alpha=1.0 and alpha=0.6 zones show obstruction, status is FULLY_OBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.8], [0.6])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert results[0].status == ObstructionStatus.FULLY_OBSTRUCTED
    assert results[0].max_obstruction_alpha1 == pytest.approx(0.8)
    assert results[0].max_obstruction_alpha06 == pytest.approx(0.6)


def test_evaluate_multiple_points(tmp_path):
    """evaluate_sample_points should return one evaluation per input point."""
    # Three points: unobstructed, partial, full
    scenarios = [
        ([0.0], [0.0]),   # unobstructed
        ([0.4], [0.0]),   # partial
        ([0.9], [0.7]),   # full
    ]

    pts = np.array([
        [1000.0, 2000.0, 50.0],
        [1100.0, 2000.0, 50.0],
        [1200.0, 2000.0, 50.0],
    ])
    common = (5000.0, 5000.0, 100.0)

    # Build a side_effect that cycles through scenarios for each point
    call_count = [0]

    def _fz_side(p, c, freq, alpha):
        return _make_zone([1200], [600])

    def _ci_side(zone, terrain):
        point_idx = call_count[0] // 2
        call_idx = call_count[0] % 2  # 0 = alpha1, 1 = alpha06
        call_count[0] += 1
        obs_1_vals, obs_06_vals = scenarios[point_idx]
        if call_idx == 0:
            return _make_obs(obs_1_vals)
        else:
            return _make_obs(obs_06_vals)

    with patch(_PATCH_FZ, side_effect=_fz_side), \
         patch(_PATCH_IT, return_value=[]), \
         patch(_PATCH_LT, return_value=_make_terrain([900])), \
         patch(_PATCH_CI, side_effect=_ci_side):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert len(results) == 3
    assert results[0].status == ObstructionStatus.UNOBSTRUCTED
    assert results[1].status == ObstructionStatus.PARTIALLY_OBSTRUCTED
    assert results[2].status == ObstructionStatus.FULLY_OBSTRUCTED


def test_evaluate_returns_correct_point_coordinates(tmp_path):
    """Each SamplePointEvaluation.point should match the corresponding input row."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pt = np.array([912650.0, 117650.0, 45.5])
    pts = pt.reshape(1, 3)
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    np.testing.assert_array_equal(results[0].point, pt)


def test_evaluate_calls_fresnel_twice_per_point(tmp_path):
    """compute_fresnel_zone should be called twice per point (alpha=1.0 and alpha=0.6)."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0], [1100.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert mock_fz.call_count == 4  # 2 points × 2 alpha values


def test_sample_point_evaluation_dataclass():
    """SamplePointEvaluation should expose expected fields."""
    pt = np.array([100.0, 200.0, 50.0])
    ev = SamplePointEvaluation(
        point=pt,
        status=ObstructionStatus.UNOBSTRUCTED,
        max_obstruction_alpha1=0.0,
        max_obstruction_alpha06=0.0,
    )
    assert ev.status == ObstructionStatus.UNOBSTRUCTED
    np.testing.assert_array_equal(ev.point, pt)
