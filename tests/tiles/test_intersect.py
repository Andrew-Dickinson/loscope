"""Tests for los_analyzer.tiles.intersect"""

import numpy as np
import pytest

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.tiles.load import TerrainGrid
from los_analyzer.lib.tiles.intersect import IntersectionGrid, compute_intersection


# ---------------------------------------------------------------------------
# Helpers
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


# ---------------------------------------------------------------------------
# Formula correctness
# ---------------------------------------------------------------------------

def test_terrain_at_bottom_gives_zero(tmp_path):
    """When terrain height equals the fresnel zone bottom, the obstruction value should be 0."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[600])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(0.0)


def test_terrain_at_top_gives_one(tmp_path):
    """When terrain height equals the fresnel zone top, the obstruction value should be 1."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[1200])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(1.0)


def test_terrain_at_midpoint_gives_half(tmp_path):
    """When terrain is halfway between bottom and top, the obstruction value should be 0.5."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[900])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(0.5)


def test_terrain_below_bottom_clips_to_zero(tmp_path):
    """When terrain is below the fresnel zone bottom, the obstruction value should be clipped to 0."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[100])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(0.0)


def test_terrain_above_top_clips_to_one(tmp_path):
    """When terrain exceeds the fresnel zone top, the obstruction value should be clipped to 1."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[65535])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(1.0)


def test_zero_span_returns_zero(tmp_path):
    """When top equals bottom (zero-height fresnel zone), the obstruction value should be 0."""
    zone = _make_zone(top_vals=[900], bottom_vals=[900])
    terrain = _make_terrain(heights=[900])
    result = compute_intersection(zone, terrain)
    assert result.values[0, 0] == pytest.approx(0.0)


def test_multiple_rows_computed_independently(tmp_path):
    """When multiple rows have different fresnel bounds, each row is computed independently."""
    zone = _make_zone(top_vals=[2400, 1200], bottom_vals=[1200, 600])
    terrain = _make_terrain(heights=[1800, 600])
    result = compute_intersection(zone, terrain)
    # Row 0: (1800-1200)/(2400-1200) = 600/1200 = 0.5
    assert result.values[0, 0] == pytest.approx(0.5)
    # Row 1: (600-600)/(1200-600) = 0
    assert result.values[1, 0] == pytest.approx(0.0)


# ---------------------------------------------------------------------------
# Output dtype and shape
# ---------------------------------------------------------------------------

def test_values_dtype_is_float32(tmp_path):
    """When the result is returned, values should be float32."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600])
    terrain = _make_terrain(heights=[900])
    result = compute_intersection(zone, terrain)
    assert result.values.dtype == np.float32


def test_values_shape_matches_input(tmp_path):
    """When computed, values shape should match (H, maxW) from the input arrays."""
    n = 5
    zone = _make_zone(top_vals=[1000] * n, bottom_vals=[0] * n)
    terrain = _make_terrain(heights=[500] * n)
    result = compute_intersection(zone, terrain)
    assert result.values.shape == (n, 1)


# ---------------------------------------------------------------------------
# Metadata copied from FresnelZone
# ---------------------------------------------------------------------------

def test_widths_offsets_copied_from_fresnel_zone(tmp_path):
    """When computed, IntersectionGrid widths and offsets should be copied from the FresnelZone."""
    widths = [3, 2, 4]
    offsets = [10, 20, 30]
    zone = _make_zone(top_vals=[1200, 1200, 1200], bottom_vals=[600, 600, 600],
                      widths=widths, offsets=offsets)
    terrain = _make_terrain(heights=[900, 900, 900], widths=widths, offsets=offsets)
    result = compute_intersection(zone, terrain)
    np.testing.assert_array_equal(result.widths, zone.widths)
    np.testing.assert_array_equal(result.offsets, zone.offsets)


def test_base_offsets_copied_from_fresnel_zone(tmp_path):
    """When computed, x_base_offset and y_base_offset should be copied from the FresnelZone."""
    zone = _make_zone(top_vals=[1200], bottom_vals=[600], x_base=1001000, y_base=236000)
    terrain = _make_terrain(heights=[900], x_base=1001000, y_base=236000)
    result = compute_intersection(zone, terrain)
    assert result.x_base_offset == 1001000
    assert result.y_base_offset == 236000


# ---------------------------------------------------------------------------
# Out-of-bounds zeroing
# ---------------------------------------------------------------------------

def test_cells_beyond_row_width_are_zero(tmp_path):
    """When a row's valid width is less than maxW, cells beyond widths[i] should be 0."""
    # Two rows, maxW=3; row 0 has width=2, row 1 has width=3
    top = np.array([[1200, 1200, 1200],
                    [1200, 1200, 1200]], dtype=np.uint16)
    bottom = np.array([[600, 600, 600],
                       [600, 600, 600]], dtype=np.uint16)
    widths = np.array([2, 3], dtype=np.uint32)
    offsets = np.zeros(2, dtype=np.uint32)
    zone = FresnelZone(top=top, bottom=bottom, widths=widths, offsets=offsets,
                       x_base_offset=0, y_base_offset=0)
    heights = np.array([[900, 900, 900],
                        [900, 900, 900]], dtype=np.uint16)
    terrain = TerrainGrid(heights=heights, widths=widths.copy(), offsets=offsets.copy(),
                          x_base_offset=0, y_base_offset=0)
    result = compute_intersection(zone, terrain)
    # Row 0: cols 0,1 valid (0.5), col 2 must be 0
    assert result.values[0, 0] == pytest.approx(0.5)
    assert result.values[0, 1] == pytest.approx(0.5)
    assert result.values[0, 2] == pytest.approx(0.0)
    # Row 1: all three cols valid
    assert result.values[1, 2] == pytest.approx(0.5)
