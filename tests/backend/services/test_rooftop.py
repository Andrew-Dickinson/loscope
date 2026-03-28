"""Tests for los_analyzer.backend.services.rooftop — build_rooftop_obj"""
import numpy as np
import pytest

# Import the full app first to resolve the circular import chain before
# importing individual service functions.
from los_analyzer.backend.app import app as _app  # noqa: F401
from los_analyzer.backend.services.rooftop import build_rooftop_obj


def test_returns_bytesio_for_nonempty_heightmap():
    heightmap = np.array([[0, 0, 0], [0, 240, 0], [0, 0, 0]], dtype=np.uint16)
    buf = build_rooftop_obj("bin_nonempty_001", heightmap)
    assert buf is not None


def test_obj_contains_header():
    heightmap = np.array([[240]], dtype=np.uint16)
    buf = build_rooftop_obj("bin_header_001", heightmap)
    content = buf.read().decode()
    assert "# Building heightmap terrain" in content
    assert "o heightmap" in content


def test_all_zero_heightmap_has_no_faces():
    """Cells with height 0 are skipped; no faces should be emitted."""
    heightmap = np.zeros((10, 10), dtype=np.uint16)
    buf = build_rooftop_obj("bin_zero_001", heightmap)
    content = buf.read().decode()
    assert "f " not in content


def test_single_nonzero_cell_produces_faces():
    heightmap = np.zeros((5, 5), dtype=np.uint16)
    heightmap[2, 2] = 240  # 20 ft
    buf = build_rooftop_obj("bin_single_001", heightmap)
    content = buf.read().decode()
    assert "f " in content


def test_cell_height_correct_in_obj():
    """A cell with value 240 (240 inches / 12 = 20 ft) should appear as z=20.000."""
    heightmap = np.array([[240]], dtype=np.uint16)
    buf = build_rooftop_obj("bin_height_001", heightmap)
    content = buf.read().decode()
    assert "20.000" in content


def test_multiple_heights_in_obj():
    heightmap = np.array([[120, 0], [0, 240]], dtype=np.uint16)
    buf = build_rooftop_obj("bin_multi_001", heightmap)
    content = buf.read().decode()
    assert "10.000" in content
    assert "20.000" in content


def test_face_count_increases_with_cells():
    """A larger heightmap with more filled cells should produce more faces."""
    small = np.full((2, 2), 120, dtype=np.uint16)
    large = np.full((10, 10), 120, dtype=np.uint16)

    small_faces = [ln for ln in build_rooftop_obj("bin_small_fc", small).read().decode().splitlines() if ln.startswith("f ")]
    large_faces = [ln for ln in build_rooftop_obj("bin_large_fc", large).read().decode().splitlines() if ln.startswith("f ")]
    assert len(large_faces) > len(small_faces)


def test_result_is_seeked_to_start():
    """The returned BytesIO should be positioned at the start so read() gives full content."""
    heightmap = np.array([[120]], dtype=np.uint16)
    buf = build_rooftop_obj("bin_seek_001", heightmap)
    content = buf.read().decode()
    assert content.startswith("#")
