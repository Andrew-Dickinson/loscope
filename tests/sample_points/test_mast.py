"""Tests for los_analyzer.sample_points.mast"""
import numpy as np
import pytest

from lib.sample_points.mast import apply_mast_offset


def pts(*rows):
    return np.array(rows, dtype=np.float64).reshape(-1, 3)


# ── basic behaviour ───────────────────────────────────────────────────────────

def test_empty_input():
    empty = np.empty((0, 3), dtype=np.float64)
    d, m = apply_mast_offset(empty, 5.0)
    assert d.shape == (0, 3)
    assert m.shape == (0, 3)


def test_zero_offset_returns_identical_arrays():
    p = pts((1, 2, 10), (1, 2, 15), (3, 4, 8))
    d, m = apply_mast_offset(p, 0.0)
    assert np.array_equal(d, p)
    assert np.array_equal(m, p)


def test_display_is_unmodified_copy():
    p = pts((1, 1, 10))
    d, m = apply_mast_offset(p, 3.0)
    assert np.array_equal(d, p)       # display unchanged
    assert not np.array_equal(m, p)   # measurement shifted


# ── single XY group ───────────────────────────────────────────────────────────

def test_single_point_shifted():
    p = pts((0, 0, 5))
    d, m = apply_mast_offset(p, 2.0)
    assert np.isclose(d[0, 2], 5.0)
    assert np.isclose(m[0, 2], 7.0)


def test_cliff_stack_only_cap_shifted():
    # Three points at same XY: z = 1, 2, 3.  Only z=3 (top) gets offset.
    p = pts((5, 5, 1), (5, 5, 2), (5, 5, 3))
    d, m = apply_mast_offset(p, 10.0)
    assert np.allclose(d[:, 2], [1, 2, 3])           # display unchanged
    assert np.allclose(m[:, 2], [1, 2, 13])           # only cap shifted


def test_non_top_points_coincident():
    p = pts((0, 0, 5), (0, 0, 10), (0, 0, 15))
    d, m = apply_mast_offset(p, 1.0)
    # Points at z=5 and z=10 are not the top — display == measurement for them
    assert np.isclose(d[0, 2], m[0, 2])
    assert np.isclose(d[1, 2], m[1, 2])
    # Top point (z=15) is shifted
    assert np.isclose(m[2, 2], 16.0)


# ── multiple XY groups ────────────────────────────────────────────────────────

def test_each_xy_group_top_shifted():
    # Two separate XY groups, each with a top point.
    p = pts(
        (0, 0, 5),   # group A — top
        (1, 1, 3),   # group B non-top
        (1, 1, 8),   # group B — top
    )
    d, m = apply_mast_offset(p, 2.0)
    assert np.isclose(m[0, 2], 7.0)   # group A top shifted
    assert np.isclose(m[1, 2], 3.0)   # group B non-top unchanged
    assert np.isclose(m[2, 2], 10.0)  # group B top shifted


def test_unique_xy_points_all_shifted():
    # Every point has a unique XY — all are their own top, all get shifted.
    p = pts((0, 0, 5), (1, 0, 3), (2, 0, 7))
    d, m = apply_mast_offset(p, 1.0)
    assert np.allclose(m[:, 2], [6, 4, 8])


# ── output properties ─────────────────────────────────────────────────────────

def test_output_dtype():
    p = pts((0, 0, 5.5))
    d, m = apply_mast_offset(p, 1.0)
    assert d.dtype == np.float64
    assert m.dtype == np.float64


def test_xy_coordinates_unchanged():
    p = pts((3.5, 7.5, 10), (3.5, 7.5, 20))
    d, m = apply_mast_offset(p, 5.0)
    assert np.allclose(m[:, :2], p[:, :2])
