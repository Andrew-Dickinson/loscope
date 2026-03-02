"""Tests for src.preprocessing.tile"""
import numpy as np
import pytest

from los_analyzer.preprocessing.tile import split_tiles, TileData
from los_analyzer.preprocessing.tile_id import GRID_N, TILE_SIDE_USFT, LAS_SIDE_USFT


ORIGIN = (1000000, 235000)
FILE_ID = "235"


@pytest.fixture
def sample_grid():
    """A 2500×2500 uint16 grid with a unique fill per 500×500 quadrant."""
    grid = np.zeros((LAS_SIDE_USFT, LAS_SIDE_USFT), dtype=np.uint16)
    for xi in range(GRID_N):
        for yi in range(GRID_N):
            grid[xi * TILE_SIDE_USFT:(xi + 1) * TILE_SIDE_USFT,
                 yi * TILE_SIDE_USFT:(yi + 1) * TILE_SIDE_USFT] = xi * GRID_N + yi
    return grid


@pytest.fixture
def tiles(sample_grid):
    return split_tiles(sample_grid, FILE_ID, ORIGIN)


def test_split_tiles_produces_25_tiles(tiles):
    """When given a 2500×2500 grid, split_tiles should produce exactly 25 TileData objects."""
    assert len(tiles) == 25


def test_each_tile_shape_is_500x500(tiles):
    """When tiles are created, each raster should have shape (500, 500)."""
    for tile in tiles:
        assert tile.raster.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)


def test_each_tile_dtype_is_uint16(tiles):
    """When tiles are created, each raster should have dtype uint16."""
    for tile in tiles:
        assert tile.raster.dtype == np.uint16


def test_nw_corner_tile_id_and_offsets(tiles):
    """When xi=0, yi=4 (NW tile), tile_id should be '235_04' and offsets should match SW corner."""
    nw = next(t for t in tiles if t.tile_id == "235_04")
    assert nw.x_offset == ORIGIN[0]
    assert nw.y_offset == ORIGIN[1] + 4 * TILE_SIDE_USFT


def test_se_corner_tile_id_and_offsets(tiles):
    """When xi=4, yi=0 (SE tile), tile_id should be '235_40' and offsets should match SW corner."""
    se = next(t for t in tiles if t.tile_id == "235_40")
    assert se.x_offset == ORIGIN[0] + 4 * TILE_SIDE_USFT
    assert se.y_offset == ORIGIN[1]


def test_tiles_have_no_overlap(tiles):
    """When tiles are created, their x/y offset pairs should all be unique (no overlap)."""
    offsets = [(t.x_offset, t.y_offset) for t in tiles]
    assert len(offsets) == len(set(offsets))


def test_tiles_cover_full_grid(tiles):
    """When 25 tiles are produced, their combined x×y coverage should span the full 2500×2500 grid."""
    # Each tile covers 500 usft in each axis; 5×5 = 25 tiles span 2500×2500
    xs = sorted(set(t.x_offset for t in tiles))
    ys = sorted(set(t.y_offset for t in tiles))
    assert len(xs) == GRID_N
    assert len(ys) == GRID_N


def test_raster_data_matches_source_grid(tiles, sample_grid):
    """When split_tiles slices the grid, each tile's raster should match the corresponding slice."""
    for tile in tiles:
        # Recover xi from x_offset
        xi = (tile.x_offset - ORIGIN[0]) // TILE_SIDE_USFT
        # Recover yi from y_offset: y_offset = origin[1] + yi*500 → yi = (y_offset - origin[1])/500
        yi = (tile.y_offset - ORIGIN[1]) // TILE_SIDE_USFT
        expected = sample_grid[xi * TILE_SIDE_USFT:(xi + 1) * TILE_SIDE_USFT,
                               yi * TILE_SIDE_USFT:(yi + 1) * TILE_SIDE_USFT]
        np.testing.assert_array_equal(tile.raster, expected)


def test_obstruction_ids_empty(tiles):
    """When tiles are created in Part 1, obstruction_ids should be an empty list."""
    for tile in tiles:
        assert tile.obstruction_ids == []
