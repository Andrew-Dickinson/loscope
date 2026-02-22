"""Tests for src.preprocessing.tile_id"""
from src.preprocessing.tile_id import (
    file_id_to_offset,
    make_tile_id,
    tile_nw_corner,
    LAS_SIDE_USFT,
    TILE_SIDE_USFT,
)


def test_file_id_to_offset_235():
    """When file_id is '235', file_id_to_offset should return (1000000, 235000)."""
    assert file_id_to_offset("235") == (1000000, 235000)


def test_file_id_to_offset_non_multiple_of_5():
    """When the numeric portion is not a multiple of 5, fname_int_to_coordinate should add 500."""
    # e.g. file_id "997240" → x portion "997", y portion "240"
    # 997 % 5 != 0 → 997 * 1000 + 500 = 997500; 997500 >= 500000 so no +1000000
    # 240 % 5 == 0 → 240 * 1000 = 240000
    x, y = file_id_to_offset("997240")
    assert x == 997500
    assert y == 240000


def test_make_tile_id_nw_corner():
    """When xi=0 and yi=4, make_tile_id should produce the '…_04' NW corner string."""
    assert make_tile_id("235", 0, 4) == "235_04"


def test_make_tile_id_se_corner():
    """When xi=4 and yi=0, make_tile_id should produce the '…_40' SE corner string."""
    assert make_tile_id("235", 4, 0) == "235_40"


def test_tile_nw_corner_nw_tile():
    """When xi=0, yi=4 (NW tile), tile_nw_corner should return origin + (0, LAS_SIDE_USFT)."""
    origin = (1000000, 235000)
    x, y = tile_nw_corner(origin, 0, 4)
    assert x == origin[0]
    assert y == origin[1] + LAS_SIDE_USFT


def test_tile_nw_corner_se_tile():
    """When xi=4, yi=0 (SE tile), tile_nw_corner should return the top-left of that tile."""
    origin = (1000000, 235000)
    x, y = tile_nw_corner(origin, 4, 0)
    assert x == origin[0] + 4 * TILE_SIDE_USFT
    assert y == origin[1] + TILE_SIDE_USFT


def test_tile_nw_corner_y_is_max_y_of_tile():
    """tile_nw_corner y-coordinate should equal the top (max Y) edge of the tile."""
    origin = (1000000, 235000)
    for yi in range(5):
        _, y = tile_nw_corner(origin, 0, yi)
        assert y == origin[1] + (yi + 1) * TILE_SIDE_USFT
