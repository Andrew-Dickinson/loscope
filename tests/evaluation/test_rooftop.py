"""Tests for los_analyzer.lib.evaluation.rooftop"""
from __future__ import annotations

from unittest.mock import MagicMock, patch

import numpy as np
import pytest

from los_analyzer.lib.evaluation.rooftop import (
    ObstructionStatus,
    SamplePointEvaluation,
    _valid_max,
    evaluate_point,
    evaluate_sample_points,
)
from los_analyzer.lib.providers.obstruction_provider import ObstructionProvider
from los_analyzer.lib.providers.tile_provider import TileProvider
from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.tiles.intersect import IntersectionGrid
from los_analyzer.lib.tiles.load import TerrainGrid


# ---------------------------------------------------------------------------
# Helpers: build minimal FresnelZone / TerrainGrid / IntersectionGrid
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
    """Build an IntersectionGrid with the given obstruction level values."""
    n = len(values_flat)
    maxW = 1
    v = np.array(values_flat, dtype=np.float32).reshape(n, maxW)
    w = np.ones(n, dtype=np.uint32) if widths is None else np.array(widths, dtype=np.uint32)
    o = np.zeros(n, dtype=np.uint32) if offsets is None else np.array(offsets, dtype=np.uint32)
    return IntersectionGrid(values=v, widths=w, offsets=o, x_base_offset=x_base, y_base_offset=y_base)


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
    obs = IntersectionGrid(values=v, widths=w, offsets=o, x_base_offset=0, y_base_offset=0)
    assert _valid_max(obs) == pytest.approx(0.0)


def test_valid_max_respects_width_mask():
    """_valid_max should ignore padding columns beyond each row's valid width."""
    # Two rows, maxW=3; row 0 has width=1, row 1 has width=2
    v = np.array([[0.9, 0.9, 0.9],
                  [0.1, 0.3, 0.9]], dtype=np.float32)
    w = np.array([1, 2], dtype=np.uint32)
    o = np.zeros(2, dtype=np.uint32)
    obs = IntersectionGrid(values=v, widths=w, offsets=o, x_base_offset=0, y_base_offset=0)
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
    assert ObstructionStatus.OBSTRUCTED.value == "obstructed"


def test_status_enum_is_str():
    """ObstructionStatus should be a str enum (values compare equal to plain strings)."""
    assert ObstructionStatus.UNOBSTRUCTED == "unobstructed"
    assert ObstructionStatus.OBSTRUCTED == "obstructed"


# ---------------------------------------------------------------------------
# evaluate_sample_points — mocked pipeline
# ---------------------------------------------------------------------------

_PATCH_FZ = "los_analyzer.lib.evaluation.rooftop.compute_fresnel_zone"
_PATCH_IT = "los_analyzer.lib.evaluation.rooftop.identify_tiles"
_PATCH_LT = "los_analyzer.lib.evaluation.rooftop.load_terrain_grid"
_PATCH_CI = "los_analyzer.lib.evaluation.rooftop.compute_intersection"


def _mock_pipeline(obs_full_vals, obs_partial_vals):
    """Return (mock_fz, mock_it, mock_lt, mock_ci) for inject into patches."""
    mock_fz = MagicMock(return_value=_make_zone([1200], [600]))
    mock_it = MagicMock(return_value=[])
    mock_lt = MagicMock(return_value=_make_terrain([900]))

    call_count = [0]
    results = [_make_obs(obs_full_vals), _make_obs(obs_partial_vals)]

    def _ci(*_args):
        idx = call_count[0] % 2
        call_count[0] += 1
        return results[idx]

    mock_ci = MagicMock(side_effect=_ci)
    return mock_fz, mock_it, mock_lt, mock_ci


def test_unobstructed_when_terrain_below_zone(tmp_path):
    """When both full and partial zones show no obstruction, status is UNOBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert len(results) == 1
    assert results[0].status == ObstructionStatus.UNOBSTRUCTED
    assert results[0].max_obstruction_full == pytest.approx(0.0)
    assert results[0].max_obstruction_partial == pytest.approx(0.0)


def test_partially_obstructed_when_terrain_in_outer_ring(tmp_path):
    """When full zone is blocked but partial is clear, status is PARTIALLY_OBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.5], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert results[0].status == ObstructionStatus.PARTIALLY_OBSTRUCTED
    assert results[0].max_obstruction_full == pytest.approx(0.5)
    assert results[0].max_obstruction_partial == pytest.approx(0.0)


def test_obstructed_when_terrain_above_partial_zone(tmp_path):
    """When both full and partial zones show obstruction, status is OBSTRUCTED."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.8], [0.6])
    pts = np.array([[1000.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert results[0].status == ObstructionStatus.OBSTRUCTED
    assert results[0].max_obstruction_full == pytest.approx(0.8)
    assert results[0].max_obstruction_partial == pytest.approx(0.6)


def test_evaluate_multiple_points(tmp_path):
    """evaluate_sample_points should return one evaluation per input point."""
    # Three points: unobstructed, partial, obstructed
    scenarios = [
        ([0.0], [0.0]),   # unobstructed
        ([0.4], [0.0]),   # partial
        ([0.9], [0.7]),   # obstructed
    ]

    pts = np.array([
        [1000.0, 2000.0, 50.0],
        [1100.0, 2000.0, 50.0],
        [1200.0, 2000.0, 50.0],
    ])
    common = (5000.0, 5000.0, 100.0)

    call_count = [0]

    def _fz_side(p, c, freq, alpha):
        return _make_zone([1200], [600])

    def _ci_side(zone, terrain, *_args):
        point_idx = call_count[0] // 2
        call_idx = call_count[0] % 2
        call_count[0] += 1
        obs_full_vals, obs_partial_vals = scenarios[point_idx]
        if call_idx == 0:
            return _make_obs(obs_full_vals)
        else:
            return _make_obs(obs_partial_vals)

    with patch(_PATCH_FZ, side_effect=_fz_side), \
         patch(_PATCH_IT, return_value=[]), \
         patch(_PATCH_LT, return_value=_make_terrain([900])), \
         patch(_PATCH_CI, side_effect=_ci_side):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert len(results) == 3
    assert results[0].status == ObstructionStatus.UNOBSTRUCTED
    assert results[1].status == ObstructionStatus.PARTIALLY_OBSTRUCTED
    assert results[2].status == ObstructionStatus.OBSTRUCTED


def test_evaluate_returns_correct_point_coordinates(tmp_path):
    """Each SamplePointEvaluation.point_a_nys should match the corresponding input row."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pt = np.array([912650.0, 117650.0, 45.5])
    pts = pt.reshape(1, 3)
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        results = evaluate_sample_points(pts, common, 24e9, tmp_path)

    np.testing.assert_array_almost_equal(results[0].point_a_nys, pt)


def test_evaluate_calls_fresnel_twice_per_point(tmp_path):
    """compute_fresnel_zone should be called twice per point (full and partial alpha)."""
    mock_fz, mock_it, mock_lt, mock_ci = _mock_pipeline([0.0], [0.0])
    pts = np.array([[1000.0, 2000.0, 50.0], [1100.0, 2000.0, 50.0]])
    common = (5000.0, 5000.0, 100.0)

    with patch(_PATCH_FZ, mock_fz), patch(_PATCH_IT, mock_it), \
         patch(_PATCH_LT, mock_lt), patch(_PATCH_CI, mock_ci):
        evaluate_sample_points(pts, common, 24e9, tmp_path)

    assert mock_fz.call_count == 4  # 2 points × 2 alpha values


def test_sample_point_evaluation_dataclass():
    """SamplePointEvaluation should expose expected fields."""
    pt_a = (100.0, 200.0, 50.0)
    pt_b = (500.0, 600.0, 80.0)
    zone = _make_zone([1200], [600])
    obs = _make_obs([0.0])
    ev = SamplePointEvaluation(
        point_a_nys=pt_a,
        point_b_nys=pt_b,
        status=ObstructionStatus.UNOBSTRUCTED,
        max_obstruction_full=0.0,
        max_obstruction_partial=0.0,
        tile_ids=[],
        frequency_hz=24e9,
        zone_full=zone,
        zone_partial=zone,
        intersection_full=obs,
        intersection_partial=obs,
    )
    assert ev.status == ObstructionStatus.UNOBSTRUCTED
    assert ev.point_a_nys == pt_a
    assert ev.point_b_nys == pt_b


# ---------------------------------------------------------------------------
# evaluate_point — mocked pipeline, specific coordinates
# ---------------------------------------------------------------------------

def test_evaluate_point_passes_coords_and_frequency_to_pipeline():
    """evaluate_point should forward pt_a, pt_b, and frequency_hz to compute_fresnel_zone."""
    pt_a = (1009748.3478422969, 253099.53772897943, 251.25)
    pt_b = (1000565.7271487191, 241854.0, 257.6095239708276)
    freq_hz = 24e9

    tile_provider = MagicMock(spec=TileProvider)
    obstruction_provider = MagicMock(spec=ObstructionProvider)

    zone = _make_zone([1200], [600])
    terrain = _make_terrain([900])
    obs = _make_obs([0.0])

    def _ci(*_args):
        return obs

    mock_fz = MagicMock(return_value=zone)

    with patch(_PATCH_FZ, mock_fz), \
         patch(_PATCH_IT, return_value=[]), \
         patch(_PATCH_LT, return_value=terrain), \
         patch(_PATCH_CI, side_effect=_ci):
        result = evaluate_point(pt_a, pt_b, freq_hz, tile_provider, obstruction_provider)

    # Fresnel zone computed twice — once per alpha
    assert mock_fz.call_count == 2
    mock_fz.assert_any_call(pt_a, pt_b, freq_hz, alpha=1.0)
    mock_fz.assert_any_call(pt_a, pt_b, freq_hz, alpha=0.6)

    # Result carries through the original coordinates and frequency
    assert result.point_a_nys == pt_a
    assert result.point_b_nys == pt_b
    assert result.frequency_hz == pytest.approx(freq_hz)
    assert result.status == ObstructionStatus.UNOBSTRUCTED
