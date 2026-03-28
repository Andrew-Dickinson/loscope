"""Tests for los_analyzer.lib.providers.tile_provider — CachingTileProvider."""

from pathlib import Path
from unittest.mock import MagicMock, patch

import numpy as np
import pytest
import tifffile

from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.lib.providers.read_through_cache import AssetProvider
from los_analyzer.lib.providers.tile_provider import (
    ASSET_TYPE_TERRAIN_TIFF,
    CachingTileProvider,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _fake_upstream(writes_tif=True):
    """Return a mock AssetProvider that optionally plants a zero-filled .tif."""

    def _get_asset(asset_type, remote_path, local_path):
        if writes_tif:
            local_path.parent.mkdir(parents=True, exist_ok=True)
            raster = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16)
            tifffile.imwrite(str(local_path), raster)
            return True
        return False

    upstream = MagicMock(spec=AssetProvider)
    upstream.get_asset.side_effect = _get_asset
    return upstream


def _make_provider(tmp_path, upstream=None):
    if upstream is None:
        upstream = _fake_upstream()
    return CachingTileProvider(upstream, tmp_path)


# ---------------------------------------------------------------------------
# Cache hit: tif already on disk
# ---------------------------------------------------------------------------

def test_tif_in_cache_not_fetched_from_upstream(tmp_path):
    """When the .tif is already in the cache dir, the upstream provider is not called."""
    raster = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16)
    tifffile.imwrite(str(tmp_path / "235_00.tif"), raster)

    upstream = _fake_upstream()
    provider = _make_provider(tmp_path, upstream)
    result = provider.get_tile("235_00")

    upstream.get_asset.assert_not_called()
    assert result is not None
    assert result.tile_id == "235_00"


def test_returns_tile_data_with_correct_offsets(tmp_path):
    """get_tile should return a TileData whose x/y offsets are derived from the tile_id."""
    raster = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16)
    tifffile.imwrite(str(tmp_path / "235_04.tif"), raster)

    provider = _make_provider(tmp_path, _fake_upstream(writes_tif=False))
    result = provider.get_tile("235_04")

    assert result is not None
    # tile_id "235_04" → xi=0, yi=4 → x=1000000, y=235000+4*500=237000
    assert result.x_offset == 1000000
    assert result.y_offset == 237000


# ---------------------------------------------------------------------------
# Cache miss: upstream is called
# ---------------------------------------------------------------------------

def test_tif_not_in_cache_fetches_from_upstream(tmp_path):
    """When the .tif is absent, the upstream provider must be called."""
    upstream = _fake_upstream(writes_tif=True)
    provider = _make_provider(tmp_path, upstream)
    result = provider.get_tile("235_00")

    upstream.get_asset.assert_called_once()
    call_args = upstream.get_asset.call_args
    assert call_args[0][0] == ASSET_TYPE_TERRAIN_TIFF
    assert "235_00.tif" in call_args[0][1]
    assert result is not None


def test_upstream_not_called_twice_for_same_tile(tmp_path):
    """Once the .tif is cached, a second get_tile call must not hit the upstream."""
    upstream = _fake_upstream(writes_tif=True)
    provider = _make_provider(tmp_path, upstream)

    provider.get_tile("235_00")  # first call — fetches from upstream and caches
    provider.get_tile("235_00")  # second call — should use cache

    assert upstream.get_asset.call_count == 1


# ---------------------------------------------------------------------------
# Missing tile upstream returns nothing
# ---------------------------------------------------------------------------

def test_upstream_returns_none_when_tile_not_found(tmp_path):
    """When the upstream has no file, get_tile returns None."""
    upstream = _fake_upstream(writes_tif=False)
    provider = _make_provider(tmp_path, upstream)
    result = provider.get_tile("235_00")
    assert result is None


# ---------------------------------------------------------------------------
# Raster shape and dtype
# ---------------------------------------------------------------------------

def test_returned_raster_shape_and_dtype(tmp_path):
    """The raster in the returned TileData should be (500, 500) uint16."""
    raster = np.arange(TILE_SIDE_USFT * TILE_SIDE_USFT, dtype=np.uint16).reshape(
        TILE_SIDE_USFT, TILE_SIDE_USFT
    )
    tifffile.imwrite(str(tmp_path / "235_00.tif"), raster)

    provider = _make_provider(tmp_path, _fake_upstream(writes_tif=False))
    result = provider.get_tile("235_00")

    assert result is not None
    assert result.raster.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)
    assert result.raster.dtype == np.uint16
    np.testing.assert_array_equal(result.raster, raster)
