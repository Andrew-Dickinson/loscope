"""Integration tests for src.preprocessing.preprocess — require data/235.las."""
import os

import numpy as np
import pytest

LAS_PATH = os.path.join(os.path.dirname(__file__), "../../data/nys_raw/235.las")

pytestmark = pytest.mark.skipif(
    not os.path.exists(LAS_PATH),
    reason="data/235.las not found",
)


@pytest.fixture(scope="module")
def tiles(tmp_path_factory):
    from lib.preprocessing.preprocess import run_preprocessing
    out = tmp_path_factory.mktemp("preprocessed")
    return run_preprocessing(LAS_PATH, out)


def test_produces_25_tiles(tiles):
    """When preprocessing runs on 235.las, it should produce exactly 25 tiles."""
    assert len(tiles) == 25


def test_all_tiles_written_as_tifs(tiles, tmp_path_factory):
    """When preprocessing runs, each tile should have a .tif on disk (no JSON)."""
    from lib.preprocessing.preprocess import run_preprocessing
    out = tmp_path_factory.mktemp("pairs")
    run_preprocessing(LAS_PATH, out)
    assert len(list(out.glob("*.tif"))) == 25
    assert len(list(out.glob("*.json"))) == 0


def test_nw_tile_has_correct_offsets(tiles):
    """When preprocessing 235.las, tile 235_04 (SW corner) should have x_offset=1000000, y_offset=237000."""
    nw = next(t for t in tiles if t.tile_id == "235_04")
    assert nw.x_offset == 1000000
    assert nw.y_offset == 237000


def test_all_rasters_are_uint16_500x500(tiles):
    """When preprocessing runs, all tile rasters should be uint16 arrays of shape (500, 500)."""
    for tile in tiles:
        assert tile.raster.dtype == np.uint16
        assert tile.raster.shape == (500, 500)
