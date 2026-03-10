"""Tests for los_analyzer.tiles.identify"""

import numpy as np
import pytest

from los_analyzer.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.tiles.identify import identify_tiles

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


def _touch(tmp_path, *tile_ids):
    """Create minimal .tif marker files for the given tile IDs."""
    for tid in tile_ids:
        (tmp_path / f"{tid}.tif").write_bytes(b"")


def test_single_tile_overlap(tmp_path):
    """When the zone falls entirely within one tile, only that tile is returned."""
    _touch(tmp_path, "235_22", "235_32")
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=100, e_width=200)
    # easting [1001100, 1001300) is inside "235_22"; "235_32" starts at 1001500
    assert identify_tiles(zone, tmp_path) == ["235_22"]


def test_zone_spans_two_easting_tiles(tmp_path):
    """When a row's easting range crosses a tile boundary, both tiles are returned."""
    _touch(tmp_path, "235_22", "235_32")
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=600)
    # easting [1001000, 1001600) overlaps both "235_22" and "235_32"
    assert sorted(identify_tiles(zone, tmp_path)) == ["235_22", "235_32"]


def test_zone_spans_two_northing_tiles(tmp_path):
    """When the zone covers rows in two northing stripes, both tiles are returned."""
    _touch(tmp_path, "235_22", "235_23")
    # northing 236450..236550 crosses the 236500 boundary between _22 and _23
    zone = _make_zone(x_base=1001000, y_base=236450, n_rows=101, e_offset=0, e_width=100)
    assert sorted(identify_tiles(zone, tmp_path)) == ["235_22", "235_23"]


def test_no_overlap_returns_empty(tmp_path):
    """When the zone does not overlap any tile, an empty list is returned."""
    _touch(tmp_path, "235_22")
    zone = _make_zone(x_base=1010000, y_base=250000, n_rows=50, e_offset=0, e_width=100)
    assert identify_tiles(zone, tmp_path) == []


def test_northing_boundary_last_row_of_tile(tmp_path):
    """When the zone's sole row is northing 236499, only the tile below 236500 is matched."""
    _touch(tmp_path, "235_22", "235_23")
    zone = _make_zone(x_base=1001000, y_base=236499, n_rows=1, e_offset=0, e_width=100)
    assert identify_tiles(zone, tmp_path) == ["235_22"]


def test_northing_boundary_first_row_of_next_tile(tmp_path):
    """When the zone's sole row is northing 236500, only the tile starting at 236500 is matched."""
    _touch(tmp_path, "235_22", "235_23")
    zone = _make_zone(x_base=1001000, y_base=236500, n_rows=1, e_offset=0, e_width=100)
    assert identify_tiles(zone, tmp_path) == ["235_23"]


def test_empty_zone_returns_empty(tmp_path):
    """When all row widths are zero, no tiles are returned."""
    _touch(tmp_path, "235_22")
    n_rows = 10
    zone = FresnelZone(
        top=np.zeros((n_rows, 1), dtype=np.uint16),
        bottom=np.zeros((n_rows, 1), dtype=np.uint16),
        widths=np.zeros(n_rows, dtype=np.uint32),
        offsets=np.zeros(n_rows, dtype=np.uint32),
        x_base_offset=1001000,
        y_base_offset=236000,
    )
    assert identify_tiles(zone, tmp_path) == []


def test_empty_directory_returns_empty(tmp_path):
    """When tile_dir contains no .tif files, an empty list is returned."""
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=100)
    assert identify_tiles(zone, tmp_path) == []


def test_result_excludes_non_overlapping_tile(tmp_path):
    """When the tile directory contains a tile not covered by the zone, it is excluded."""
    _touch(tmp_path, "235_22", "235_23", "235_32")
    # Zone only covers "235_22" northing and easting ranges
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=100)
    result = identify_tiles(zone, tmp_path)
    assert "235_22" in result
    assert "235_23" not in result
    assert "235_32" not in result


# --- require_exists=False mode (no filesystem needed) ---

def test_require_exists_false_single_tile():
    """When require_exists=False, identify_tiles returns the tile ID without needing any files."""
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=100, e_width=200)
    assert identify_tiles(zone, require_exists=False) == ["235_22"]


def test_require_exists_false_spans_two_easting_tiles():
    """When require_exists=False and the zone spans two easting tiles, both IDs are returned."""
    zone = _make_zone(x_base=1001000, y_base=236100, n_rows=50, e_offset=0, e_width=600)
    assert sorted(identify_tiles(zone, require_exists=False)) == ["235_22", "235_32"]


def test_require_exists_false_spans_two_northing_tiles():
    """When require_exists=False and the zone crosses a northing boundary, both IDs are returned."""
    zone = _make_zone(x_base=1001000, y_base=236450, n_rows=101, e_offset=0, e_width=100)
    assert sorted(identify_tiles(zone, require_exists=False)) == ["235_22", "235_23"]


def test_require_exists_false_matches_directory_mode(tmp_path):
    """When require_exists=False, results match the directory-based mode for the same zone."""
    # Zone spans E[1001000,1001600) x N[236450,236550): touches all four corner tiles.
    _touch(tmp_path, "235_22", "235_23", "235_32", "235_33")
    zone = _make_zone(x_base=1001000, y_base=236450, n_rows=101, e_offset=0, e_width=600)
    assert sorted(identify_tiles(zone, tmp_path)) == sorted(identify_tiles(zone, require_exists=False))


@pytest.mark.parametrize("easting,northing,expected_tile", [
    # 235_10: xi=1,yi=0, origin(1000000,235000), covers E[1000500,1001000) N[235000,235500)
    (1000582, 235200, "235_10"),
    # 997235_04: xi=0,yi=4, origin(997500,235000), covers E[997500,998000) N[237000,237500)
    (997700,  237001, "997235_04"),
    # 997235_41: xi=4,yi=1, origin(997500,235000), covers E[999500,1000000) N[235500,236000)
    (999758,  235999, "997235_41"),
    (1007888,  228580, "7227_02"),
])
def test_require_exists_false_real_coordinates(easting, northing, expected_tile):
    """When require_exists=False, a single-point zone maps to the expected real tile."""
    zone = _make_zone(x_base=easting, y_base=northing, n_rows=1, e_offset=0, e_width=1)
    assert identify_tiles(zone, require_exists=False) == [expected_tile]


def test_require_exists_false_empty_zone():
    """When require_exists=False and the zone is empty, an empty list is returned."""
    n_rows = 10
    zone = FresnelZone(
        top=np.zeros((n_rows, 1), dtype=np.uint16),
        bottom=np.zeros((n_rows, 1), dtype=np.uint16),
        widths=np.zeros(n_rows, dtype=np.uint32),
        offsets=np.zeros(n_rows, dtype=np.uint32),
        x_base_offset=1001000,
        y_base_offset=236000,
    )
    assert identify_tiles(zone, require_exists=False) == []
