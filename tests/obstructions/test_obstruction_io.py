"""Tests for src.los_analyzer.obstructions.io"""
import numpy as np
import pytest

from los_analyzer.lib.obstructions.io import load_obstruction, save_obstruction
from los_analyzer.lib.obstructions.model import OBSTRUCTION_TYPE_BUILDING, Obstruction


@pytest.fixture
def sample_obstruction():
    raster = np.array([[100, 200], [150, 0]], dtype=np.uint16)
    return Obstruction(
        obstruction_id="test-uuid-1234",
        obstruction_type=OBSTRUCTION_TYPE_BUILDING,
        attributes={"BIN": "1234567", "ground_elevation": 10.0},
        x_offset=950000,
        y_offset=180000,
        raster=raster,
        tile_ids=["235_00", "235_01"],
    )


def test_save_creates_tif_and_json(tmp_path, sample_obstruction):
    """When save_obstruction is called, it should create a .tif and a .json file."""
    save_obstruction(sample_obstruction, tmp_path)
    assert (tmp_path / "test-uuid-1234.tif").exists()
    assert (tmp_path / "test-uuid-1234.json").exists()


def test_roundtrip_preserves_raster(tmp_path, sample_obstruction):
    """When saved and reloaded, the raster data should be identical."""
    save_obstruction(sample_obstruction, tmp_path)
    loaded = load_obstruction("test-uuid-1234", tmp_path)
    np.testing.assert_array_equal(loaded.raster, sample_obstruction.raster)


def test_roundtrip_preserves_metadata(tmp_path, sample_obstruction):
    """When saved and reloaded, all metadata fields should match."""
    save_obstruction(sample_obstruction, tmp_path)
    loaded = load_obstruction("test-uuid-1234", tmp_path)
    assert loaded.obstruction_id == sample_obstruction.obstruction_id
    assert loaded.obstruction_type == sample_obstruction.obstruction_type
    assert loaded.x_offset == sample_obstruction.x_offset
    assert loaded.y_offset == sample_obstruction.y_offset
    assert loaded.attributes == sample_obstruction.attributes


def test_json_is_human_readable(tmp_path, sample_obstruction):
    """When saved, the JSON file should be indented (human-readable)."""
    save_obstruction(sample_obstruction, tmp_path)
    text = (tmp_path / "test-uuid-1234.json").read_text()
    assert "\n" in text  # indented JSON has newlines


def test_tile_ids_preserved_after_roundtrip(tmp_path, sample_obstruction):
    """When saved and reloaded, tile_ids should be identical."""
    save_obstruction(sample_obstruction, tmp_path)
    loaded = load_obstruction("test-uuid-1234", tmp_path)
    assert loaded.tile_ids == sample_obstruction.tile_ids


def test_tile_ids_is_top_level_json_property(tmp_path, sample_obstruction):
    """When saved, tile_ids should appear as a top-level key in the JSON file."""
    import json
    save_obstruction(sample_obstruction, tmp_path)
    meta = json.loads((tmp_path / "test-uuid-1234.json").read_text())
    assert "tile_ids" in meta
    assert meta["tile_ids"] == ["235_00", "235_01"]


def test_raster_dtype_preserved_after_roundtrip(tmp_path, sample_obstruction):
    """When saved and reloaded, the raster dtype should still be uint16."""
    save_obstruction(sample_obstruction, tmp_path)
    loaded = load_obstruction("test-uuid-1234", tmp_path)
    assert loaded.raster.dtype == np.uint16
