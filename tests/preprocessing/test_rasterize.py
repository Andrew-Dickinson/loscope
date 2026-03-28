"""Tests for src.preprocessing.rasterize"""
import numpy as np

from los_analyzer.lib.preprocessing.rasterize import fill_gaps


def _make_grids(shape=(100, 100)):
    height_grid = np.zeros(shape, dtype=np.float64)
    data_count = np.zeros(shape, dtype=np.int32)
    return height_grid, data_count


def test_fill_gaps_preserves_data_pixels():
    """When a pixel has data, fill_gaps should preserve its original height (in inches)."""
    height_grid, data_count = _make_grids()
    # Surround center with data so median_filter would give a different value
    height_grid[50:53, 50:53] = 50.0
    data_count[50:53, 50:53] = 1
    # Override center pixel with a distinct value
    height_grid[51, 51] = 60.0
    data_count[51, 51] = 1

    result = fill_gaps(height_grid, data_count)

    # Center pixel had data_count > 0 so its original value must be preserved
    assert result[51, 51] == round(60.0 * 12)  # 720 inches


def test_fill_gaps_fills_no_data_pixels():
    """When a pixel has no data but is surrounded by data, fill_gaps should fill it from the median filter."""
    height_grid, data_count = _make_grids()
    # 3×3 block of 50 usft with a hole at the center
    height_grid[50:53, 50:53] = 50.0
    data_count[50:53, 50:53] = 1
    # Remove data at the center pixel
    height_grid[51, 51] = 0.0
    data_count[51, 51] = 0

    result = fill_gaps(height_grid, data_count)

    # Center was surrounded by 50 usft → median = 50 usft → 600 inches
    assert result[51, 51] == round(50.0 * 12)  # 600 inches


def test_fill_gaps_clips_to_uint16_max():
    """When heights exceed uint16 max (in inches), fill_gaps should clip to 65535."""
    height_grid, data_count = _make_grids()
    height_grid[0, 0] = 100000.0  # absurdly tall → 1 200 000 inches, well above 65535
    data_count[0, 0] = 1

    result = fill_gaps(height_grid, data_count)

    assert result[0, 0] == 65535


def test_fill_gaps_returns_uint16():
    """fill_gaps should always return an array with dtype uint16."""
    height_grid, data_count = _make_grids()
    result = fill_gaps(height_grid, data_count)
    assert result.dtype == np.uint16
