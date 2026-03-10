"""Tests for src.preprocessing.io"""
import json

import numpy as np
import pytest
import tifffile

from los_analyzer.preprocessing.io import save_tile, load_tile
from los_analyzer.preprocessing.tile import TileData
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT


@pytest.fixture
def sample_tile():
    raster = np.arange(TILE_SIDE_USFT * TILE_SIDE_USFT, dtype=np.uint16).reshape(
        TILE_SIDE_USFT, TILE_SIDE_USFT
    )
    return TileData(
        tile_id="235_04",
        x_offset=1000000,
        y_offset=237500,
        raster=raster,
    )


def test_save_tile_creates_tif_and_json(tmp_path, sample_tile):
    """When save_tile is called, it should create both a .tif and a .json file."""
    save_tile(sample_tile, tmp_path)
    assert (tmp_path / "235_04.tif").exists()
    assert (tmp_path / "235_04.json").exists()


def test_saved_json_has_required_fields(tmp_path, sample_tile):
    """When save_tile is called, the JSON file should contain tile_id, x_offset, y_offset, raster_file."""
    save_tile(sample_tile, tmp_path)
    data = json.loads((tmp_path / "235_04.json").read_text())
    assert data["tile_id"] == "235_04"
    assert data["x_offset"] == 1000000
    assert data["y_offset"] == 237500
    assert data["raster_file"] == "235_04.tif"
    assert "obstruction_ids" not in data


def test_tif_roundtrip_preserves_dtype_and_values(tmp_path, sample_tile):
    """When a tile is saved and reloaded, the raster dtype and values should be identical."""
    save_tile(sample_tile, tmp_path)
    loaded = tifffile.imread(str(tmp_path / "235_04.tif"))
    assert loaded.dtype == np.uint16
    np.testing.assert_array_equal(loaded, sample_tile.raster)


def test_load_tile_roundtrip(tmp_path, sample_tile):
    """When load_tile is called after save_tile, the returned TileData should match the original."""
    save_tile(sample_tile, tmp_path)
    loaded = load_tile("235_04", tmp_path)
    assert loaded.tile_id == sample_tile.tile_id
    assert loaded.x_offset == sample_tile.x_offset
    assert loaded.y_offset == sample_tile.y_offset
    np.testing.assert_array_equal(loaded.raster, sample_tile.raster)
