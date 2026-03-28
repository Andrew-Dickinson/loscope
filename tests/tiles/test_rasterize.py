"""Tests for los_analyzer.lib.tiles.rasterize"""
from __future__ import annotations

import numpy as np
import pytest

from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.lib.tiles.rasterize import rasterize_stairstep_grid_for_tile


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_grid(n_rows: int, width: int, x_base: int, y_base: int, value=0):
    """Build uniform stairstep arrays with a constant fill value."""
    widths = np.full(n_rows, width, dtype=np.uint32)
    offsets = np.zeros(n_rows, dtype=np.uint32)
    values = np.full((n_rows, max(width, 1)), value, dtype=np.uint16)
    return widths, offsets, values, (x_base, y_base)


# ---------------------------------------------------------------------------
# Output shape
# ---------------------------------------------------------------------------

def test_output_shape_is_tile_size():
    """Output shape should always be (TILE_SIDE_USFT, TILE_SIDE_USFT)."""
    widths, offsets, values, base = _make_grid(5, 10, 1001000, 236000)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert result.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)


def test_output_dtype_matches_input():
    """Output dtype should match the values array dtype."""
    widths, offsets, values, base = _make_grid(5, 10, 1001000, 236000, value=7)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert result.dtype == np.uint16


# ---------------------------------------------------------------------------
# No overlap
# ---------------------------------------------------------------------------

def test_returns_zeros_when_grid_outside_tile_easting():
    """When the grid is far east of the tile, output is all zeros."""
    # tile "235_22": x=1001000..1001500, y=236000..236500
    widths, offsets, values, base = _make_grid(10, 100, 1010000, 236000, value=5)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert (result == 0).all()


def test_returns_zeros_when_grid_outside_tile_northing():
    """When the grid is entirely above the tile northing range, output is all zeros."""
    widths, offsets, values, base = _make_grid(10, 100, 1001000, 250000, value=5)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert (result == 0).all()


# ---------------------------------------------------------------------------
# Zero rows and zero values
# ---------------------------------------------------------------------------

def test_zero_width_rows_produce_no_output():
    """Rows with width=0 should be skipped entirely."""
    n_rows = 10
    widths = np.zeros(n_rows, dtype=np.uint32)
    offsets = np.zeros(n_rows, dtype=np.uint32)
    values = np.zeros((n_rows, 1), dtype=np.uint16)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, (1001000, 236000))
    assert (result == 0).all()


def test_zero_value_pixels_not_written():
    """Pixels with value 0 in the grid should not appear in the output."""
    widths, offsets, values, base = _make_grid(5, 10, 1001000, 236000, value=0)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert (result == 0).all()


# ---------------------------------------------------------------------------
# Correct blitting
# ---------------------------------------------------------------------------

def test_full_overlap_fills_expected_region():
    """When the grid exactly covers part of the tile, those cells are filled."""
    # tile "235_22": x=1001000, y=236000
    # Grid: 5 rows starting at y=236000, 10 wide starting at x=1001000
    widths, offsets, values, base = _make_grid(5, 10, 1001000, 236000, value=99)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, base)
    assert (result[:5, :10] == 99).all()
    assert (result[5:, :] == 0).all()
    assert (result[:5, 10:] == 0).all()


def test_partial_easting_overlap_fills_only_overlap():
    """When the grid extends beyond the tile in easting, only the overlap is filled."""
    # Grid starts at x=1000800, 400 wide: covers [1000800, 1001200)
    # Tile starts at x=1001000: overlap is [1001000, 1001200) → cols [0, 200)
    n_rows = 5
    widths = np.full(n_rows, 400, dtype=np.uint32)
    offsets = np.zeros(n_rows, dtype=np.uint32)
    values = np.full((n_rows, 400), 7, dtype=np.uint16)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, (1000800, 236000))
    assert (result[:n_rows, :200] == 7).all()
    assert (result[:n_rows, 200:] == 0).all()


def test_nonzero_offset_shifts_columns():
    """A nonzero offset should shift where grid values land in the tile."""
    # Grid: 1 row, base=(1001000, 236000), offset=50, width=10 → covers [1001050, 1001060)
    widths = np.array([10], dtype=np.uint32)
    offsets = np.array([50], dtype=np.uint32)
    values = np.full((1, 10), 42, dtype=np.uint16)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, (1001000, 236000))
    assert result[0, 50] == 42
    assert result[0, 49] == 0
    assert result[0, 60] == 0


def test_grid_starting_below_tile_skips_non_overlapping_rows():
    """When the grid starts below the tile SW corner, only overlapping rows fill the output."""
    # tile "235_22": y=236000. Grid: 13 rows starting at y=235990.
    # Rows 0-9 are below the tile; rows 10-12 overlap → land at tile rows 0-2.
    n_rows = 13
    widths = np.full(n_rows, 5, dtype=np.uint32)
    offsets = np.zeros(n_rows, dtype=np.uint32)
    values = np.full((n_rows, 5), 33, dtype=np.uint16)
    result = rasterize_stairstep_grid_for_tile("235_22", widths, offsets, values, (1001000, 235990))
    assert (result[0:3, :5] == 33).all()
    assert (result[3:, :5] == 0).all()
