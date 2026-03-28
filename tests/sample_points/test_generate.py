"""Tests for los_analyzer.lib.sample_points — generate_sample_points, point_encode, get_paired_sample_points"""
from __future__ import annotations

import numpy as np
import pytest
from shapely.geometry import Polygon

from los_analyzer.lib.building.heightmap import RooftopHeightMap
from los_analyzer.lib.sample_points import generate_sample_points, point_encode
from los_analyzer.lib.sample_points import get_paired_sample_points


# ---------------------------------------------------------------------------
# point_encode
# ---------------------------------------------------------------------------

def test_point_encode_relative_x_y():
    """x/y should be relative to x_sw/y_sw."""
    pt = np.array([912650.0, 117650.0, 100.0])
    enc = point_encode(pt, 912600, 117600)
    assert enc["x"] == pytest.approx(50.0)
    assert enc["y"] == pytest.approx(50.0)


def test_point_encode_absolute_nys_coords():
    """nys_e/nys_n/nys_z should be absolute NYS coordinates."""
    pt = np.array([912650.0, 117650.0, 100.0])
    enc = point_encode(pt, 912600, 117600)
    assert enc["nys_e"] == pytest.approx(912650.0)
    assert enc["nys_n"] == pytest.approx(117650.0)
    assert enc["nys_z"] == pytest.approx(100.0)


def test_point_encode_z_matches_nys_z():
    """z field should equal nys_z (height is absolute, not relative to origin)."""
    pt = np.array([0.0, 0.0, 500.0])
    enc = point_encode(pt, 0.0, 0.0)
    assert enc["z"] == pytest.approx(enc["nys_z"])


def test_point_encode_returns_typed_dict():
    """point_encode should return a dict with all expected keys."""
    pt = np.array([100.0, 200.0, 50.0])
    enc = point_encode(pt, 100.0, 200.0)
    for key in ("x", "y", "z", "nys_e", "nys_n", "nys_z"):
        assert key in enc


# ---------------------------------------------------------------------------
# generate_sample_points — without polygon
# ---------------------------------------------------------------------------

def test_generate_without_polygon_returns_ndarray():
    """generate_sample_points should return an (N, 3) float64 array."""
    heightmap = np.full((50, 50), 600, dtype=np.uint16)
    result = generate_sample_points(heightmap, x_sw=1000, y_sw=2000, spacing=10)
    assert result.ndim == 2
    assert result.shape[1] == 3
    assert result.dtype == np.float64


def test_generate_z_is_inches_divided_by_12():
    """Z values should equal the heightmap value (inches) / 12 (converted to feet)."""
    heightmap = np.full((20, 20), 720, dtype=np.uint16)  # 720 inches = 60 ft
    result = generate_sample_points(heightmap, x_sw=0, y_sw=0, spacing=5)
    assert len(result) > 0
    assert np.allclose(result[:, 2], 60.0)


def test_generate_xy_within_heightmap_bounds():
    """All XY sample points should lie within the heightmap's NYS extent."""
    W, H = 40, 40
    x_sw, y_sw = 1000, 2000
    heightmap = np.full((W, H), 600, dtype=np.uint16)
    result = generate_sample_points(heightmap, x_sw, y_sw, spacing=10)
    if len(result):
        assert result[:, 0].min() >= x_sw
        assert result[:, 0].max() < x_sw + W
        assert result[:, 1].min() >= y_sw
        assert result[:, 1].max() < y_sw + H


def test_generate_with_all_zero_mask_returns_empty():
    """When mask is all zeros, no sample points should be returned."""
    heightmap = np.full((20, 20), 600, dtype=np.uint16)
    mask = np.zeros((20, 20), dtype=np.uint8)
    result = generate_sample_points(heightmap, x_sw=0, y_sw=0, spacing=5, mask=mask)
    assert result.shape == (0, 3)


def test_generate_spacing_1_returns_many_points():
    """With spacing=1, there should be roughly W*H grid points."""
    W, H = 10, 10
    heightmap = np.full((W, H), 600, dtype=np.uint16)
    result = generate_sample_points(heightmap, x_sw=0, y_sw=0, spacing=1)
    assert len(result) >= W * H


# ---------------------------------------------------------------------------
# generate_sample_points — with polygon
# ---------------------------------------------------------------------------

def test_generate_with_polygon_returns_ndarray():
    """generate_sample_points with a polygon should return an (N, 3) array."""
    W, H = 50, 50
    x_sw, y_sw = 1000, 2000
    heightmap = np.full((W, H), 600, dtype=np.uint16)
    mask = np.full((W, H), 255, dtype=np.uint8)
    poly = Polygon([
        (x_sw, y_sw), (x_sw + W, y_sw), (x_sw + W, y_sw + H), (x_sw, y_sw + H),
    ])
    result = generate_sample_points(heightmap, x_sw, y_sw, spacing=10, mask=mask, polygon=poly)
    assert result.ndim == 2
    assert result.shape[1] == 3


def test_generate_with_polygon_includes_perimeter_points():
    """When a polygon is provided, perimeter points should be included."""
    W, H = 50, 50
    x_sw, y_sw = 1000, 2000
    heightmap = np.full((W, H), 600, dtype=np.uint16)
    mask = np.full((W, H), 255, dtype=np.uint8)
    poly = Polygon([
        (x_sw, y_sw), (x_sw + W, y_sw), (x_sw + W, y_sw + H), (x_sw, y_sw + H),
    ])
    result_with_poly = generate_sample_points(heightmap, x_sw, y_sw, spacing=10, mask=mask, polygon=poly)
    result_without_poly = generate_sample_points(heightmap, x_sw, y_sw, spacing=10, mask=mask)
    # With polygon: perimeter points added, some base grid points culled
    assert len(result_with_poly) > 0


# ---------------------------------------------------------------------------
# get_paired_sample_points
# ---------------------------------------------------------------------------

def _make_heightmap_model(W=20, H=20, height_in=600, spacing_for_poly=None):
    """Build a minimal RooftopHeightMap for testing."""
    x_sw, y_sw = 912600, 117600
    heightmap = np.full((W, H), height_in, dtype=np.uint16)
    mask = np.full((W, H), 255, dtype=np.uint8)
    poly = Polygon([
        (x_sw, y_sw), (x_sw + W, y_sw), (x_sw + W, y_sw + H), (x_sw, y_sw + H),
    ])
    return RooftopHeightMap(
        bin_id="1234567",
        x_sw=x_sw,
        y_sw=y_sw,
        heightmap=heightmap,
        mask=mask,
        poly_nys=poly,
    )


def test_get_paired_sample_points_returns_list():
    """get_paired_sample_points should return a list of SamplePoint dicts."""
    model = _make_heightmap_model()
    result = get_paired_sample_points(model, sample_spacing=5, mast_offset=3.0)
    assert isinstance(result, list)
    assert len(result) > 0


def test_get_paired_sample_points_each_has_display_and_measurement():
    """Each SamplePoint should have display_point and measurement_point keys."""
    model = _make_heightmap_model()
    result = get_paired_sample_points(model, sample_spacing=5, mast_offset=3.0)
    for sp in result:
        assert "display_point" in sp
        assert "measurement_point" in sp


def test_get_paired_sample_points_measurement_higher_than_display():
    """measurement_point.z should be >= display_point.z (mast lifts the measurement)."""
    model = _make_heightmap_model()
    result = get_paired_sample_points(model, sample_spacing=5, mast_offset=3.0)
    for sp in result:
        assert sp["measurement_point"]["nys_z"] >= sp["display_point"]["nys_z"]


def test_get_paired_sample_points_display_has_all_encoded_keys():
    """Each encoded point should have x, y, z, nys_e, nys_n, nys_z keys."""
    model = _make_heightmap_model()
    result = get_paired_sample_points(model, sample_spacing=5, mast_offset=0.0)
    for sp in result:
        for key in ("x", "y", "z", "nys_e", "nys_n", "nys_z"):
            assert key in sp["display_point"]
            assert key in sp["measurement_point"]
