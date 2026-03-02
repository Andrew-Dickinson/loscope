"""Tests for los_analyzer.tiles.fetch — CachingTileFetcher and s3_fetcher_from_env."""

from pathlib import Path

import pytest

from los_analyzer.tiles.fetch import CachingTileFetcher, TileBackend, s3_fetcher_from_env


# ---------------------------------------------------------------------------
# Fake backend (no I/O, just records calls and plants expected files)
# ---------------------------------------------------------------------------

class FakeBackend:
    def __init__(self):
        self.calls: list[str] = []

    def fetch_tile_files(self, tile_id: str, dest_dir: Path) -> None:
        self.calls.append(tile_id)
        (dest_dir / f"{tile_id}.tif").write_bytes(b"")
        (dest_dir / f"{tile_id}.json").write_text("{}")


# ---------------------------------------------------------------------------
# Protocol conformance
# ---------------------------------------------------------------------------

def test_fake_backend_satisfies_protocol():
    assert isinstance(FakeBackend(), TileBackend)


# ---------------------------------------------------------------------------
# CachingTileFetcher.is_cached
# ---------------------------------------------------------------------------

def test_is_cached_false_when_both_missing(tmp_path):
    assert not CachingTileFetcher(FakeBackend(), tmp_path).is_cached("235_00")


def test_is_cached_false_when_only_tif_present(tmp_path):
    (tmp_path / "235_00.tif").write_bytes(b"")
    assert not CachingTileFetcher(FakeBackend(), tmp_path).is_cached("235_00")


def test_is_cached_false_when_only_json_present(tmp_path):
    (tmp_path / "235_00.json").write_text("{}")
    assert not CachingTileFetcher(FakeBackend(), tmp_path).is_cached("235_00")


def test_is_cached_true_when_both_present(tmp_path):
    (tmp_path / "235_00.tif").write_bytes(b"")
    (tmp_path / "235_00.json").write_text("{}")
    assert CachingTileFetcher(FakeBackend(), tmp_path).is_cached("235_00")


# ---------------------------------------------------------------------------
# CachingTileFetcher.ensure_tile
# ---------------------------------------------------------------------------

def test_ensure_tile_calls_backend_when_not_cached(tmp_path):
    backend = FakeBackend()
    CachingTileFetcher(backend, tmp_path).ensure_tile("235_00")
    assert backend.calls == ["235_00"]


def test_ensure_tile_skips_backend_when_already_cached(tmp_path):
    (tmp_path / "235_00.tif").write_bytes(b"")
    (tmp_path / "235_00.json").write_text("{}")
    backend = FakeBackend()
    CachingTileFetcher(backend, tmp_path).ensure_tile("235_00")
    assert backend.calls == []


def test_ensure_tile_creates_cache_dir_if_missing(tmp_path):
    cache_dir = tmp_path / "nested" / "cache"
    CachingTileFetcher(FakeBackend(), cache_dir).ensure_tile("235_00")
    assert cache_dir.is_dir()


def test_ensure_tile_called_twice_only_fetches_once(tmp_path):
    backend = FakeBackend()
    fetcher = CachingTileFetcher(backend, tmp_path)
    fetcher.ensure_tile("235_00")
    fetcher.ensure_tile("235_00")
    assert backend.calls == ["235_00"]


# ---------------------------------------------------------------------------
# CachingTileFetcher.ensure_tiles
# ---------------------------------------------------------------------------

def test_ensure_tiles_fetches_all_missing(tmp_path):
    backend = FakeBackend()
    CachingTileFetcher(backend, tmp_path).ensure_tiles(["235_00", "235_01"])
    assert sorted(backend.calls) == ["235_00", "235_01"]


def test_ensure_tiles_skips_cached_fetches_missing(tmp_path):
    (tmp_path / "235_00.tif").write_bytes(b"")
    (tmp_path / "235_00.json").write_text("{}")
    backend = FakeBackend()
    CachingTileFetcher(backend, tmp_path).ensure_tiles(["235_00", "235_01"])
    assert backend.calls == ["235_01"]


def test_ensure_tiles_empty_list_is_noop(tmp_path):
    backend = FakeBackend()
    CachingTileFetcher(backend, tmp_path).ensure_tiles([])
    assert backend.calls == []


# ---------------------------------------------------------------------------
# s3_fetcher_from_env
# ---------------------------------------------------------------------------

def test_s3_fetcher_from_env_reads_env_vars(tmp_path, monkeypatch):
    monkeypatch.setenv("LOS_S3_BUCKET", "my-bucket")
    monkeypatch.setenv("LOS_S3_PREFIX", "some/prefix")
    fetcher = s3_fetcher_from_env(tmp_path)
    assert fetcher.backend.bucket == "my-bucket"
    assert fetcher.backend.prefix == "some/prefix"
    assert fetcher.cache_dir == tmp_path


def test_s3_fetcher_from_env_missing_bucket_raises(tmp_path, monkeypatch):
    monkeypatch.delenv("LOS_S3_BUCKET", raising=False)
    monkeypatch.setenv("LOS_S3_PREFIX", "some/prefix")
    with pytest.raises(KeyError):
        s3_fetcher_from_env(tmp_path)


def test_s3_fetcher_from_env_missing_prefix_raises(tmp_path, monkeypatch):
    monkeypatch.setenv("LOS_S3_BUCKET", "my-bucket")
    monkeypatch.delenv("LOS_S3_PREFIX", raising=False)
    with pytest.raises(KeyError):
        s3_fetcher_from_env(tmp_path)
