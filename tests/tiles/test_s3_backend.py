"""Tests for los_analyzer.tiles.s3_backend — S3TileBackend."""

from pathlib import Path
from unittest.mock import MagicMock, call, patch

import pytest

from los_analyzer.tiles.s3_backend import S3TileBackend


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _backend_with_mock_client(bucket, prefix):
    """Return (backend, mock_client) with boto3.client patched."""
    mock_client = MagicMock()
    patcher = patch("boto3.client", return_value=mock_client)
    patcher.start()
    backend = S3TileBackend(bucket, prefix)
    return backend, mock_client, patcher


# ---------------------------------------------------------------------------
# fetch_tile_files
# ---------------------------------------------------------------------------

def test_downloads_tif_and_json(tmp_path):
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        S3TileBackend("my-bucket", "nyc/preprocessed").fetch_tile_files("235_00", tmp_path)

        mock_boto.assert_called_once_with("s3")
        assert mock_client.download_file.call_count == 2
        calls = mock_client.download_file.call_args_list
        assert call("my-bucket", "nyc/preprocessed/235_00.tif", str(tmp_path / "235_00.tif")) in calls
        assert call("my-bucket", "nyc/preprocessed/235_00.json", str(tmp_path / "235_00.json")) in calls


def test_trailing_slash_in_prefix_is_normalized(tmp_path):
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        S3TileBackend("my-bucket", "nyc/preprocessed/").fetch_tile_files("235_00", tmp_path)

        for c in mock_client.download_file.call_args_list:
            key = c.args[1]
            assert "//" not in key, f"double slash in key: {key!r}"


def test_key_format_uses_prefix_slash_tileid_dot_ext(tmp_path):
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        S3TileBackend("bucket", "prefix").fetch_tile_files("10140_00", tmp_path)

        keys = {c.args[1] for c in mock_client.download_file.call_args_list}
        assert keys == {"prefix/10140_00.tif", "prefix/10140_00.json"}


def test_dest_paths_are_inside_dest_dir(tmp_path):
    with patch("boto3.client") as mock_boto:
        mock_client = MagicMock()
        mock_boto.return_value = mock_client

        S3TileBackend("bucket", "prefix").fetch_tile_files("235_00", tmp_path)

        dest_paths = {c.args[2] for c in mock_client.download_file.call_args_list}
        assert dest_paths == {
            str(tmp_path / "235_00.tif"),
            str(tmp_path / "235_00.json"),
        }


# ---------------------------------------------------------------------------
# Prefix normalisation
# ---------------------------------------------------------------------------

def test_prefix_stored_without_trailing_slash():
    backend = S3TileBackend("b", "some/prefix/")
    assert backend.prefix == "some/prefix"


def test_prefix_without_trailing_slash_unchanged():
    backend = S3TileBackend("b", "some/prefix")
    assert backend.prefix == "some/prefix"
