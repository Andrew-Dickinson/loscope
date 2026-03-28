"""Tests for los_analyzer.lib.providers.read_through_cache — AwsS3AssetProvider."""

from pathlib import Path
from unittest.mock import MagicMock, call, patch

import pytest

from los_analyzer.lib.providers.read_through_cache import AwsS3AssetProvider
from los_analyzer.lib.providers.tile_provider import ASSET_TYPE_TERRAIN_TIFF


# ---------------------------------------------------------------------------
# Construction
# ---------------------------------------------------------------------------

def test_bucket_name_stored():
    with patch("boto3.client"):
        p = AwsS3AssetProvider("my-bucket", {ASSET_TYPE_TERRAIN_TIFF: "tiles"})
    assert p.bucket_name == "my-bucket"


def test_asset_type_prefixes_stored():
    with patch("boto3.client"):
        p = AwsS3AssetProvider("b", {ASSET_TYPE_TERRAIN_TIFF: "some/prefix"})
    assert p.asset_type_prefixes[ASSET_TYPE_TERRAIN_TIFF] == "some/prefix"


# ---------------------------------------------------------------------------
# get_asset — success path
# ---------------------------------------------------------------------------

def test_get_asset_calls_download_file(tmp_path):
    """get_asset should call client.download_file with the correct bucket/key/dest."""
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("my-bucket", {ASSET_TYPE_TERRAIN_TIFF: "tiles"})
        dest = tmp_path / "235_00.tif"
        result = provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "235_00.tif", dest)

    mock_client.download_file.assert_called_once_with(
        "my-bucket", "tiles/235_00.tif", str(dest)
    )
    assert result is True


def test_get_asset_uses_prefix_for_key(tmp_path):
    """The S3 key must be <prefix>/<asset_name>."""
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "nyc/preprocessed"})
        provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "235_00.tif", tmp_path / "235_00.tif")

    key = mock_client.download_file.call_args[0][1]
    assert key == "nyc/preprocessed/235_00.tif"


def test_get_asset_dest_path_used_as_local_path(tmp_path):
    """get_asset should download to the exact local_asset_path provided."""
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "p"})
        dest = tmp_path / "sub" / "235_00.tif"
        provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "235_00.tif", dest)

    actual_dest = mock_client.download_file.call_args[0][2]
    assert actual_dest == str(dest)


# ---------------------------------------------------------------------------
# get_asset — 404 / not-found
# ---------------------------------------------------------------------------

def test_get_asset_returns_false_on_404(tmp_path):
    """When S3 returns a 404, get_asset should return False without raising."""
    from botocore.exceptions import ClientError

    error_response = {"Error": {"Code": "404", "Message": "Not Found"}}

    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_client.download_file.side_effect = ClientError(error_response, "GetObject")
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "p"})
        result = provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "missing.tif", tmp_path / "missing.tif")

    assert result is False


def test_get_asset_returns_false_on_no_such_key(tmp_path):
    """When S3 returns NoSuchKey, get_asset should return False without raising."""
    from botocore.exceptions import ClientError

    error_response = {"Error": {"Code": "NoSuchKey", "Message": "The specified key does not exist."}}

    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_client.download_file.side_effect = ClientError(error_response, "GetObject")
        mock_boto.return_value = mock_client

        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "p"})
        result = provider.get_asset(ASSET_TYPE_TERRAIN_TIFF, "missing.tif", tmp_path / "missing.tif")

    assert result is False


# ---------------------------------------------------------------------------
# get_asset — unknown asset type
# ---------------------------------------------------------------------------

def test_get_asset_raises_on_unknown_asset_type(tmp_path):
    """When the asset_type is not in asset_type_prefixes, get_asset should raise KeyError."""
    with patch("boto3.client"):
        provider = AwsS3AssetProvider("bucket", {ASSET_TYPE_TERRAIN_TIFF: "p"})

    with pytest.raises(KeyError):
        provider.get_asset("UNKNOWN_TYPE", "file.tif", tmp_path / "file.tif")
