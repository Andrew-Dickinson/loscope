"""Tests for los_analyzer.sample_points.combine"""
import numpy as np
import pytest

from lib.sample_points.combine import cull_and_combine


def pts(*rows):
    """Build float64 (N, 3) from row tuples."""
    return np.array(rows, dtype=np.float64).reshape(-1, 3)


EMPTY = np.empty((0, 3), dtype=np.float64)


# ── no perimeter ──────────────────────────────────────────────────────────────

def test_no_perim_returns_base_and_cliff():
    base = pts((1, 1, 10), (2, 2, 10))
    cliff = pts((1, 1, 15))
    result = cull_and_combine(base, cliff, EMPTY, cull_radius=5.0)
    assert len(result) == 3


def test_no_perim_no_cliff_returns_base():
    base = pts((1, 1, 10))
    result = cull_and_combine(base, EMPTY, EMPTY, cull_radius=5.0)
    assert len(result) == 1
    assert np.allclose(result[0], [1, 1, 10])


def test_all_empty_returns_empty():
    result = cull_and_combine(EMPTY, EMPTY, EMPTY, cull_radius=5.0)
    assert result.shape == (0, 3)


# ── culling ───────────────────────────────────────────────────────────────────

def test_base_far_from_perim_not_culled():
    base = pts((100, 100, 10))
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, EMPTY, perim, cull_radius=5.0)
    # distance = sqrt(100²+100²) ≈ 141 >> 5 → not culled
    assert any(np.allclose(r, [100, 100, 10]) for r in result)


def test_base_close_to_perim_is_culled():
    base = pts((1, 0, 10))   # distance to perim point = 1 < spacing=5
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, EMPTY, perim, cull_radius=5.0)
    # base culled; only perim point remains
    assert len(result) == 1
    assert np.allclose(result[0], [0, 0, 5])


def test_base_at_exact_spacing_not_culled():
    # dist == spacing → NOT culled (condition is strictly <)
    base = pts((5, 0, 10))
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, EMPTY, perim, cull_radius=5.0)
    assert any(np.allclose(r, [5, 0, 10]) for r in result)


def test_mixed_culled_and_kept():
    base = pts((1, 0, 10), (10, 0, 10))  # first culled, second kept
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, EMPTY, perim, cull_radius=5.0)
    xs = set(result[:, 0].tolist())
    assert 10.0 in xs        # kept
    assert 1.0 not in xs     # culled


# ── cliff points never culled ─────────────────────────────────────────────────

def test_cliff_kept_when_base_culled():
    # base at (1,0) is within spacing=5 of perim at (0,0) → base culled.
    # cliff at same XY (1,0) must NOT be culled.
    base = pts((1, 0, 5))
    cliff = pts((1, 0, 15), (1, 0, 25))  # stacked above culled base
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, cliff, perim, cull_radius=5.0)
    cliff_rows = result[np.isclose(result[:, 0], 1.0)]
    assert len(cliff_rows) == 2
    assert set(cliff_rows[:, 2].tolist()) == {15.0, 25.0}


def test_cliff_kept_when_base_also_kept():
    base = pts((10, 0, 5))
    cliff = pts((10, 0, 15))
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, cliff, perim, cull_radius=5.0)
    assert len(result) == 3  # perim + base + cliff


def test_cliff_only_no_base():
    cliff = pts((3, 3, 20))
    perim = pts((3, 3, 5))   # same XY as cliff — but cliff is never culled
    result = cull_and_combine(EMPTY, cliff, perim, cull_radius=5.0)
    assert len(result) == 2
    zs = set(result[:, 2].tolist())
    assert 20.0 in zs   # cliff kept


# ── output properties ─────────────────────────────────────────────────────────

def test_output_dtype():
    base = pts((1, 1, 10))
    perim = pts((0, 0, 5))
    result = cull_and_combine(base, EMPTY, perim, cull_radius=5.0)
    assert result.dtype == np.float64


def test_output_columns():
    base = pts((5, 5, 10))
    result = cull_and_combine(base, EMPTY, EMPTY, cull_radius=5.0)
    assert result.shape[1] == 3
