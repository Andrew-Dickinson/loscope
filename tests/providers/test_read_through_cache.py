"""Tests for los_analyzer.lib.providers.read_through_cache — ReadThroughCache."""
from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest

from los_analyzer.lib.providers.read_through_cache import (
    AssetProvider,
    AwsS3AssetProvider,
    ReadThroughCache,
)
from los_analyzer.lib.providers.tile_provider import ASSET_TYPE_TERRAIN_TIFF


# ---------------------------------------------------------------------------
# Concrete subclass for testing
# ---------------------------------------------------------------------------

class _Cache(ReadThroughCache):
    pass


def _upstream(write_file=True, write_dir=True):
    """Return a mock AssetProvider that optionally writes a file/folder on demand."""
    up = MagicMock(spec=AssetProvider)

    def _get_asset(asset_type, remote_path, local_path):
        if write_file:
            local_path.parent.mkdir(parents=True, exist_ok=True)
            local_path.write_bytes(b"data")
            return True
        return False

    def _sync_path(asset_type, remote_path, local_path):
        if write_dir:
            local_path.mkdir(parents=True, exist_ok=True)
            (local_path / "index.json").write_text("{}")
            return True
        return False

    up.get_asset.side_effect = _get_asset
    up.sync_path.side_effect = _sync_path
    return up


# ---------------------------------------------------------------------------
# get_from_fs_cache_or_upstream
# ---------------------------------------------------------------------------

def test_cache_hit_returns_path_without_calling_upstream(tmp_path):
    """When the file already exists in cache, upstream is never called."""
    (tmp_path / "tile.tif").write_bytes(b"cached")
    up = _upstream()
    cache = _Cache(up, tmp_path)
    result = cache.get_from_fs_cache_or_upstream("TERRAIN", "tile.tif")
    assert result == tmp_path / "tile.tif"
    up.get_asset.assert_not_called()


def test_cache_miss_calls_upstream(tmp_path):
    """When the file is absent, upstream.get_asset is called."""
    up = _upstream(write_file=True)
    cache = _Cache(up, tmp_path)
    result = cache.get_from_fs_cache_or_upstream("TERRAIN", "tile.tif")
    assert result is not None
    up.get_asset.assert_called_once()


def test_cache_miss_returns_none_when_upstream_has_nothing(tmp_path):
    """When upstream also returns False, get_from_fs_cache_or_upstream returns None."""
    up = _upstream(write_file=False)
    cache = _Cache(up, tmp_path)
    result = cache.get_from_fs_cache_or_upstream("TERRAIN", "tile.tif")
    assert result is None


def test_second_call_uses_cached_file(tmp_path):
    """After a cache miss fetches from upstream, a second call must not hit upstream again."""
    up = _upstream(write_file=True)
    cache = _Cache(up, tmp_path)
    cache.get_from_fs_cache_or_upstream("TERRAIN", "tile.tif")
    cache.get_from_fs_cache_or_upstream("TERRAIN", "tile.tif")
    assert up.get_asset.call_count == 1


# ---------------------------------------------------------------------------
# get_folder_from_fs_cache_or_upstream
# ---------------------------------------------------------------------------

def test_folder_hit_returns_path_without_calling_upstream(tmp_path):
    """When the folder already exists, upstream.sync_path is never called."""
    folder = tmp_path / "_indexes"
    folder.mkdir()
    up = _upstream()
    cache = _Cache(up, tmp_path)
    result = cache.get_folder_from_fs_cache_or_upstream("OBS_IDX", "_indexes")
    assert result == folder
    up.sync_path.assert_not_called()


def test_folder_miss_calls_upstream_sync(tmp_path):
    """When folder is absent, upstream.sync_path is called."""
    up = _upstream(write_dir=True)
    cache = _Cache(up, tmp_path)
    result = cache.get_folder_from_fs_cache_or_upstream("OBS_IDX", "_indexes")
    assert result is not None
    up.sync_path.assert_called_once()


def test_folder_miss_returns_none_when_upstream_finds_nothing(tmp_path):
    """When upstream sync returns False, get_folder_from_fs_cache_or_upstream returns None."""
    up = _upstream(write_dir=False)
    cache = _Cache(up, tmp_path)
    result = cache.get_folder_from_fs_cache_or_upstream("OBS_IDX", "_indexes")
    assert result is None


# ---------------------------------------------------------------------------
# AwsS3AssetProvider — sync_path
# ---------------------------------------------------------------------------

def test_sync_path_downloads_all_objects(tmp_path):
    """sync_path should download every object under the S3 prefix and return True."""
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        # Simulate paginator returning two objects
        pages = [{"Contents": [{"Key": "pfx/_indexes/a.json"}, {"Key": "pfx/_indexes/b.json"}]}]
        paginator = MagicMock()
        paginator.paginate.return_value = iter(pages)
        mock_client.get_paginator.return_value = paginator

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "pfx"})
        dest = tmp_path / "_indexes"
        result = provider.sync_path(ASSET_TYPE_TERRAIN_TIFF, "_indexes", dest)

    assert result is True
    assert mock_client.download_file.call_count == 2


def test_sync_path_returns_false_when_prefix_empty(tmp_path):
    """sync_path should return False when the S3 prefix has no objects."""
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        pages = [{}]  # no "Contents" key
        paginator = MagicMock()
        paginator.paginate.return_value = iter(pages)
        mock_client.get_paginator.return_value = paginator

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "pfx"})
        result = provider.sync_path(ASSET_TYPE_TERRAIN_TIFF, "_indexes", tmp_path / "_indexes")

    assert result is False


# ---------------------------------------------------------------------------
# AwsS3AssetProvider — non-404 error re-raise
# ---------------------------------------------------------------------------

def test_get_asset_reraises_non_404_client_error(tmp_path):
    """When S3 returns a non-404 ClientError, get_asset should re-raise it."""
    from botocore.exceptions import ClientError

    error_response = {"Error": {"Code": "403", "Message": "Forbidden"}}
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_client.download_file.side_effect = ClientError(error_response, "GetObject")
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "pfx"})
        with pytest.raises(ClientError):
            provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "file.tif", tmp_path / "file.tif")


def test_sync_path_raises_on_unknown_asset_type(tmp_path):
    """sync_path should raise KeyError when asset_type is not configured."""
    with patch("boto3.client"):
        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "pfx"})
    with pytest.raises(KeyError):
        provider.sync_path("UNKNOWN_TYPE", "_indexes", tmp_path / "_indexes")
