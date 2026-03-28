"""Tests for src.preprocessing.io"""
import numpy as np
import tifffile

from los_analyzer.lib.preprocessing.io import save_tile, load_tile
from los_analyzer.lib.preprocessing.tile import TileData
from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT


def _sample_tile():
    raster = np.arange(TILE_SIDE_USFT * TILE_SIDE_USFT, dtype=np.uint16).reshape(
        TILE_SIDE_USFT, TILE_SIDE_USFT
    )
    return TileData(
        tile_id="235_04",
        x_offset=1000000,
        y_offset=237000,
        raster=raster,
    )


def test_save_tile_creates_tif(tmp_path):
    """When save_tile is called, it should create a .tif file and no .json file."""
    tile = _sample_tile()
    save_tile(tile, tmp_path)
    assert (tmp_path / "235_04.tif").exists()
    assert not (tmp_path / "235_04.json").exists()


def test_tif_roundtrip_preserves_dtype_and_values(tmp_path):
    """When a tile is saved and reloaded, the raster dtype and values should be identical."""
    tile = _sample_tile()
    save_tile(tile, tmp_path)
    loaded = tifffile.imread(str(tmp_path / "235_04.tif"))
    assert loaded.dtype == np.uint16
    np.testing.assert_array_equal(loaded, tile.raster)


def test_load_tile_roundtrip(tmp_path):
    """When load_tile is called after save_tile, the returned TileData should match the original."""
    tile = _sample_tile()
    save_tile(tile, tmp_path)
    loaded = load_tile("235_04", tmp_path)
    assert loaded.tile_id == tile.tile_id
    assert loaded.x_offset == 1000000
    assert loaded.y_offset == 237000
    np.testing.assert_array_equal(loaded.raster, tile.raster)


def test_load_tile_derives_offsets_from_tile_id(tmp_path):
    """load_tile should compute x_offset and y_offset from the tile ID without any JSON file."""
    tile = _sample_tile()
    save_tile(tile, tmp_path)
    # Verify no JSON exists — offsets must come purely from the tile ID.
    assert not (tmp_path / "235_04.json").exists()
    loaded = load_tile("235_04", tmp_path)
    assert loaded.x_offset == 1000000
    assert loaded.y_offset == 237000
