"""Tests for los_analyzer.lib.obstructions.visualize"""
import numpy as np
import pytest

from los_analyzer.lib.obstructions.model import Obstruction, OBSTRUCTION_TYPE_BUILDING
from los_analyzer.lib.obstructions.visualize import create_obstruction_obj

# tile "235_00": e0=1000000, n0=235000, extends to e=1000500, n=235500
_TILE_ID = "235_00"
_TILE_E0 = 1000000
_TILE_N0 = 235000


def _make_obs(raster, x_offset=1000100, y_offset=235100, obs_id="test-obs-001"):
    return Obstruction(
        obstruction_id=obs_id,
        obstruction_type=OBSTRUCTION_TYPE_BUILDING,
        attributes={},
        x_offset=x_offset,
        y_offset=y_offset,
        raster=raster,
        tile_ids=[_TILE_ID],
    )


def test_returns_bytesio_for_overlapping_obstruction():
    raster = np.array([[240]], dtype=np.uint16)
    obs = _make_obs(raster)
    assert create_obstruction_obj(obs, _TILE_ID) is not None


def test_obj_contains_comment_header():
    raster = np.array([[240]], dtype=np.uint16)
    obs = _make_obs(raster)
    buf = create_obstruction_obj(obs, _TILE_ID)
    content = buf.read().decode()
    assert "# Obstruction volume mesh" in content
    assert "1 unit = 1 US survey foot" in content


def test_obj_contains_object_name():
    raster = np.array([[240]], dtype=np.uint16)
    obs = _make_obs(raster, obs_id="test-obs-001")
    buf = create_obstruction_obj(obs, _TILE_ID)
    content = buf.read().decode()
    assert "o obstruction_" in content


def test_obj_contains_vertices_and_faces_for_nonzero_cell():
    raster = np.array([[240]], dtype=np.uint16)
    obs = _make_obs(raster)
    content = create_obstruction_obj(obs, _TILE_ID).read().decode()
    assert "v " in content
    assert "f " in content


def test_top_face_at_correct_height():
    """A cell with value 240 (= 20.000 ft) should produce z=20.000 in the OBJ."""
    raster = np.array([[240]], dtype=np.uint16)  # 240 inches / 12 = 20 ft
    obs = _make_obs(raster)
    content = create_obstruction_obj(obs, _TILE_ID).read().decode()
    assert "20.000" in content


def test_returns_none_when_obstruction_outside_tile():
    """Obstruction entirely outside the tile boundary should return None."""
    raster = np.array([[240]], dtype=np.uint16)
    obs = _make_obs(raster, x_offset=1200000, y_offset=400000)
    assert create_obstruction_obj(obs, _TILE_ID) is None


def test_all_zero_raster_produces_no_faces():
    """A raster of all zeros has no elevated cells, so no faces should be emitted."""
    raster = np.zeros((5, 5), dtype=np.uint16)
    obs = _make_obs(raster)
    content = create_obstruction_obj(obs, _TILE_ID).read().decode()
    assert "f " not in content


def test_multi_cell_raster_produces_multiple_heights():
    """Different cell heights should appear as different z values in the OBJ."""
    raster = np.array([[120, 0], [0, 240]], dtype=np.uint16)  # 10 ft and 20 ft
    obs = _make_obs(raster)
    content = create_obstruction_obj(obs, _TILE_ID).read().decode()
    assert "10.000" in content
    assert "20.000" in content


def test_large_raster_fully_inside_tile():
    """A 50×50 raster fully inside the tile should produce substantial OBJ output."""
    raster = np.full((50, 50), 120, dtype=np.uint16)  # 10 ft everywhere
    obs = _make_obs(raster)
    content = create_obstruction_obj(obs, _TILE_ID).read().decode()
    face_lines = [ln for ln in content.splitlines() if ln.startswith("f ")]
    assert len(face_lines) > 50
