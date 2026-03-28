"""Tests for los_analyzer.lib.providers.obstruction_provider — CachingObstructionProvider."""
from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock

import numpy as np
import pytest
import tifffile

from los_analyzer.lib.obstructions.model import Obstruction, OBSTRUCTION_TYPE_BUILDING
from los_analyzer.lib.providers.obstruction_provider import (
    CachingObstructionProvider,
)
from los_analyzer.lib.providers.read_through_cache import AssetProvider


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _noop_upstream():
    """Upstream that always reports assets missing (cache must be pre-populated)."""
    up = MagicMock(spec=AssetProvider)
    up.get_asset.return_value = False
    up.sync_path.return_value = False
    return up


def _write_obstruction_to_cache(cache_dir: Path, obs: Obstruction) -> None:
    """Write an Obstruction's .json and .tif files into the cache directory."""
    obs_dir = cache_dir / obs.obstruction_type
    obs_dir.mkdir(parents=True, exist_ok=True)

    # Write .tif
    tif_path = obs_dir / f"{obs.obstruction_id}.tif"
    tifffile.imwrite(str(tif_path), obs.raster)

    # Write .json metadata
    json_path = obs_dir / f"{obs.obstruction_id}.json"
    json_path.write_text(json.dumps({
        "obstruction_id": obs.obstruction_id,
        "obstruction_type": obs.obstruction_type,
        "attributes": obs.attributes,
        "tile_ids": obs.tile_ids,
        "x_offset": obs.x_offset,
        "y_offset": obs.y_offset,
    }))


def _write_index(cache_dir: Path, obs_type: str, index: dict) -> None:
    """Write a tile-to-obstruction-IDs index .json file in _indexes/."""
    idx_dir = cache_dir / "_indexes"
    idx_dir.mkdir(parents=True, exist_ok=True)
    (idx_dir / f"{obs_type}.json").write_text(json.dumps(index))


def _make_obs(obs_id="obs-001", obs_type=OBSTRUCTION_TYPE_BUILDING) -> Obstruction:
    return Obstruction(
        obstruction_id=obs_id,
        obstruction_type=obs_type,
        attributes={"BIN": "123"},
        x_offset=1001000,
        y_offset=236000,
        raster=np.full((10, 10), 500, dtype=np.uint16),
        tile_ids=["235_22"],
    )


# ---------------------------------------------------------------------------
# get_obstruction
# ---------------------------------------------------------------------------

def test_get_obstruction_returns_none_when_files_missing(tmp_path):
    """When neither detail nor raster is in cache or upstream, get_obstruction returns None."""
    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.get_obstruction(OBSTRUCTION_TYPE_BUILDING, "missing-id")
    assert result is None


def test_get_obstruction_returns_obstruction_from_cache(tmp_path):
    """When files are pre-cached, get_obstruction reconstructs the Obstruction."""
    obs = _make_obs()
    _write_obstruction_to_cache(tmp_path, obs)

    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.get_obstruction(obs.obstruction_type, obs.obstruction_id)

    assert result is not None
    assert result.obstruction_id == obs.obstruction_id
    assert result.obstruction_type == obs.obstruction_type


def test_get_obstruction_raster_matches_original(tmp_path):
    """Reconstructed raster should equal the original."""
    obs = _make_obs()
    _write_obstruction_to_cache(tmp_path, obs)

    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.get_obstruction(obs.obstruction_type, obs.obstruction_id)

    np.testing.assert_array_equal(result.raster, obs.raster)


def test_get_obstruction_attributes_preserved(tmp_path):
    """Attributes dict should survive the round-trip."""
    obs = _make_obs()
    _write_obstruction_to_cache(tmp_path, obs)

    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.get_obstruction(obs.obstruction_type, obs.obstruction_id)

    assert result.attributes == obs.attributes


def test_get_obstruction_offsets_correct(tmp_path):
    """x_offset and y_offset should survive the round-trip."""
    obs = _make_obs()
    _write_obstruction_to_cache(tmp_path, obs)

    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.get_obstruction(obs.obstruction_type, obs.obstruction_id)

    assert result.x_offset == obs.x_offset
    assert result.y_offset == obs.y_offset


# ---------------------------------------------------------------------------
# obstruction_ids_for_tile_id
# ---------------------------------------------------------------------------

def test_obstruction_ids_raises_when_no_index(tmp_path):
    """When _indexes folder is not available, obstruction_ids_for_tile_id raises FileNotFoundError."""
    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    with pytest.raises(FileNotFoundError, match="indexes"):
        provider.obstruction_ids_for_tile_id("235_22")


def test_obstruction_ids_returns_empty_for_unknown_tile(tmp_path):
    """When the tile_id has no entries in any index, an empty dict is returned."""
    _write_index(tmp_path, OBSTRUCTION_TYPE_BUILDING, {"235_33": ["obs-001"]})
    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.obstruction_ids_for_tile_id("235_22")
    assert result == {}


def test_obstruction_ids_returns_ids_for_matching_tile(tmp_path):
    """When a tile_id matches an index entry, the obstruction IDs are returned."""
    _write_index(tmp_path, OBSTRUCTION_TYPE_BUILDING, {"235_22": ["obs-001", "obs-002"]})
    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.obstruction_ids_for_tile_id("235_22")
    assert OBSTRUCTION_TYPE_BUILDING in result
    assert sorted(result[OBSTRUCTION_TYPE_BUILDING]) == ["obs-001", "obs-002"]


def test_obstruction_ids_merges_multiple_type_indexes(tmp_path):
    """When multiple type-index files exist, all are merged in the result."""
    _write_index(tmp_path, OBSTRUCTION_TYPE_BUILDING, {"235_22": ["obs-001"]})
    _write_index(tmp_path, "manual_annotation", {"235_22": ["ann-001"]})
    provider = CachingObstructionProvider(_noop_upstream(), tmp_path)
    result = provider.obstruction_ids_for_tile_id("235_22")
    assert OBSTRUCTION_TYPE_BUILDING in result
    assert "manual_annotation" in result
