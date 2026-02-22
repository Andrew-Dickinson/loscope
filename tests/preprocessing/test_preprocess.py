"""Integration tests for src.preprocessing.preprocess — require data/235.las."""
import os

import numpy as np
import pytest

LAS_PATH = os.path.join(os.path.dirname(__file__), "../../data/235.las")

pytestmark = pytest.mark.skipif(
    not os.path.exists(LAS_PATH),
    reason="data/235.las not found",
)


@pytest.fixture(scope="module")
def tiles(tmp_path_factory):
    from los_analyzer.preprocessing.preprocess import run_preprocessing
    out = tmp_path_factory.mktemp("preprocessed")
    return run_preprocessing(LAS_PATH, out)


def test_produces_25_tiles(tiles):
    """When preprocessing runs on 235.las, it should produce exactly 25 tiles."""
    assert len(tiles) == 25


def test_all_tiles_written_as_pairs(tiles, tmp_path_factory):
    """When preprocessing runs, each tile should have a matching .tif and .json on disk."""
    from los_analyzer.preprocessing.preprocess import run_preprocessing
    out = tmp_path_factory.mktemp("pairs")
    run_preprocessing(LAS_PATH, out)
    for ext in ("tif", "json"):
        files = list(out.glob(f"*.{ext}"))
        assert len(files) == 25


def test_nw_tile_has_correct_offsets(tiles):
    """When preprocessing 235.las, tile 235_04 (NW corner) should have x_offset=1000000, y_offset=237500."""
    nw = next(t for t in tiles if t.tile_id == "235_04")
    assert nw.x_offset == 1000000
    assert nw.y_offset == 237500


def test_all_rasters_are_uint16_500x500(tiles):
    """When preprocessing runs, all tile rasters should be uint16 arrays of shape (500, 500)."""
    for tile in tiles:
        assert tile.raster.dtype == np.uint16
        assert tile.raster.shape == (500, 500)
