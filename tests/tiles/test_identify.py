"""Tests for los_analyzer.lib.tiles.identify"""

import numpy as np
import pytest

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.tiles.identify import identify_tiles

# Tile coordinate reference (file_id="235", origin=(1000000, 235000), TILE_SIDE=500):
#   "235_22"  xi=2, yi=2 → SW (1001000, 236000), covers E[1001000,1001500) N[236000,236500)
#   "235_32"  xi=3, yi=2 → SW (1001500, 236000), covers E[1001500,1002000) N[236000,236500)
#   "235_23"  xi=2, yi=3 → SW (1001000, 236500), covers E[1001000,1001500) N[236500,237000)


def _make_zone(x_base, y_base, n_rows, e_offset, e_width):
    """Build a FresnelZone with uniform row dimensions."""
    widths = np.full(n_rows, e_width, dtype=np.uint32)
    offsets = np.full(n_rows, e_offset, dtype=np.uint32)
    top = np.zeros((n_rows, max(e_width, 1)), dtype=np.uint16)
    bottom = np.zeros((n_rows, max(e_width, 1)), dtype=np.uint16)
    return FresnelZone(top=top, bottom=bottom, widths=widths, offsets=offsets,
                       x_base_offset=x_base, y_base_offset=y_base)


def test_single_tile_overlap():
    """When the zone falls entirely within one tile, only that tile is returned."""
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=100, e_width=200)
    # easting [1001100, 1001300) is inside "235_22"; "235_32" starts at 1001500
    assert identify_tiles(zone) == ["235_22"]


def test_zone_spans_two_easting_tiles():
    """When a row's easting range crosses a tile boundary, both tiles are returned."""
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=600)
    # easting [1001000, 1001600) overlaps both "235_22" and "235_32"
    assert sorted(identify_tiles(zone)) == ["235_22", "235_32"]


def test_zone_spans_two_northing_tiles():
    """When the zone covers rows in two northing stripes, both tiles are returned."""
    # northing 236450..236550 crosses the 236500 boundary between _22 and _23
    zone = _make_zone(x_base=1001000, y_base=236450, n_rows=101, e_offset=0, e_width=100)
    assert sorted(identify_tiles(zone)) == ["235_22", "235_23"]


def test_northing_boundary_last_row_of_tile():
    """When the zone's sole row is northing 236499, only the tile below 236500 is matched."""
    zone = _make_zone(x_base=1001000, y_base=236499, n_rows=1, e_offset=0, e_width=100)
    assert identify_tiles(zone) == ["235_22"]


def test_northing_boundary_first_row_of_next_tile():
    """When the zone's sole row is northing 236500, only the tile starting at 236500 is matched."""
    zone = _make_zone(x_base=1001000, y_base=236500, n_rows=1, e_offset=0, e_width=100)
    assert identify_tiles(zone) == ["235_23"]


def test_empty_zone_returns_empty():
    """When all row widths are zero, no tiles are returned."""
    n_rows = 10
    zone = FresnelZone(
        top=np.zeros((n_rows, 1), dtype=np.uint16),
        bottom=np.zeros((n_rows, 1), dtype=np.uint16),
        widths=np.zeros(n_rows, dtype=np.uint32),
        offsets=np.zeros(n_rows, dtype=np.uint32),
        x_base_offset=1001000,
        y_base_offset=236000,
    )
    assert identify_tiles(zone) == []


def test_result_excludes_non_overlapping_tile():
    """When a tile does not overlap the zone, it is excluded from the result."""
    # Zone only covers "235_22" northing and easting ranges
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=100)
    result = identify_tiles(zone)
    assert "235_22" in result
    assert "235_23" not in result
    assert "235_32" not in result


def test_zone_spans_all_four_corner_tiles():
    """When the zone spans both northing and easting boundaries, all four corner tiles are returned."""
    zone = _make_zone(x_base=1001000, y_base=236450, n_rows=101, e_offset=0, e_width=600)
    result = sorted(identify_tiles(zone))
    assert result == ["235_22", "235_23", "235_32", "235_33"]


@pytest.mark.parametrize("easting,northing,expected_tile", [
    # 235_10: xi=1,yi=0, origin(1000000,235000), covers E[1000500,1001000) N[235000,235500)
    (1000582, 235200, "235_10"),
    # 997235_04: xi=0,yi=4, origin(997500,235000), covers E[997500,998000) N[237000,237500)
    (997700,  237001, "997235_04"),
    # 997235_41: xi=4,yi=1, origin(997500,235000), covers E[999500,1000000) N[235500,236000)
    (999758,  235999, "997235_41"),
    (1007888,  228580, "7227_02"),
])
def test_real_coordinates(easting, northing, expected_tile):
    """A single-point zone maps to the expected real tile."""
    zone = _make_zone(x_base=easting, y_base=northing, n_rows=1, e_offset=0, e_width=1)
    assert identify_tiles(zone) == [expected_tile]
