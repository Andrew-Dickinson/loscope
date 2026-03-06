"""Tests for los_analyzer.dem.preprocess_dem"""
import json

import numpy as np
import pytest
import tifffile

from los_analyzer.dem.preprocess_dem import (
    DEM_ORIGIN_E,
    DEM_ORIGIN_N,
    dem_tile_sw_corners,
    extract_dem_tile,
    split_dem,
)
from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def tiny_dem():
    """A 1000×1000 DEM (rows×cols) where each pixel value equals its column index
    (i.e. easting offset from DEM_ORIGIN_E), in US survey feet.  This makes it
    easy to verify both orientation and value conversion in assertions."""
    h, w = 1000, 1000
    dem = np.tile(np.arange(w, dtype=np.float32), (h, 1))
    return dem


# ---------------------------------------------------------------------------
# dem_tile_sw_corners
# ---------------------------------------------------------------------------

def test_sw_corners_are_500_aligned(tiny_dem):
    """When a DEM is given, every returned SW corner should be a multiple of 500."""
    corners = dem_tile_sw_corners(*tiny_dem.shape)
    for e, n in corners:
        assert e % TILE_SIDE_USFT == 0, f"easting {e} not 500-aligned"
        assert n % TILE_SIDE_USFT == 0, f"northing {n} not 500-aligned"


def test_sw_corners_cover_full_dem_extent(tiny_dem):
    """When a 1000×1000 DEM is given, the returned corners should span the full extent."""
    h, w = tiny_dem.shape
    corners = dem_tile_sw_corners(h, w)
    # DEM easting:  [DEM_ORIGIN_E, DEM_ORIGIN_E + 1000)
    # DEM northing: [DEM_ORIGIN_N - 1000, DEM_ORIGIN_N)
    max_e = max(e for e, _ in corners)
    min_n = min(n for _, n in corners)
    assert max_e + TILE_SIDE_USFT >= DEM_ORIGIN_E + w
    assert min_n <= DEM_ORIGIN_N - h


def test_sw_corners_west_edge_at_or_west_of_dem_origin(tiny_dem):
    """When the DEM origin easting is not 500-aligned, the westernmost tile should
    start west of DEM_ORIGIN_E."""
    h, w = tiny_dem.shape
    corners = dem_tile_sw_corners(h, w)
    min_e = min(e for e, _ in corners)
    assert min_e <= DEM_ORIGIN_E
    assert DEM_ORIGIN_E - min_e < TILE_SIDE_USFT


# ---------------------------------------------------------------------------
# extract_dem_tile
# ---------------------------------------------------------------------------

def test_extract_fully_interior_tile_shape(tiny_dem):
    """When a tile lies fully inside the DEM, extract_dem_tile should return a
    (500, 500) uint16 array."""
    # Pick a tile whose SW corner is well inside the DEM.
    e_sw = (DEM_ORIGIN_E // TILE_SIDE_USFT) * TILE_SIDE_USFT + TILE_SIDE_USFT
    n_sw = (DEM_ORIGIN_N - tiny_dem.shape[0]) // TILE_SIDE_USFT * TILE_SIDE_USFT + TILE_SIDE_USFT
    raster = extract_dem_tile(tiny_dem, e_sw, n_sw)
    assert raster is not None
    assert raster.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)
    assert raster.dtype == np.uint16


def test_extract_returns_none_outside_dem(tiny_dem):
    """When a tile is entirely west of the DEM, extract_dem_tile should return None."""
    e_sw = DEM_ORIGIN_E - TILE_SIDE_USFT  # fully west
    n_sw = DEM_ORIGIN_N - TILE_SIDE_USFT
    assert extract_dem_tile(tiny_dem, e_sw, n_sw) is None


def test_extract_returns_none_north_of_dem(tiny_dem):
    """When a tile is entirely north of the DEM, extract_dem_tile should return None."""
    e_sw = DEM_ORIGIN_E
    n_sw = DEM_ORIGIN_N  # tile spans [DEM_ORIGIN_N, DEM_ORIGIN_N+500), above DEM
    assert extract_dem_tile(tiny_dem, e_sw, n_sw) is None


def test_extract_axis_orientation(tiny_dem):
    """When extract_dem_tile returns a raster, raster[i, j] should correspond to
    easting (e_sw + i) and northing (n_sw + j)."""
    # tiny_dem pixel value equals its column = easting offset from DEM_ORIGIN_E.
    # So height at (e_sw + i, n_sw + j) = (e_sw + i) - DEM_ORIGIN_E = e_sw - DEM_ORIGIN_E + i.
    # In inches: round((e_sw - DEM_ORIGIN_E + i) * 12).

    # Use a tile fully inside the DEM.
    e_sw = DEM_ORIGIN_E + TILE_SIDE_USFT     # starts 500 east of DEM origin
    n_sw = DEM_ORIGIN_N - 2 * TILE_SIDE_USFT # well inside vertically

    raster = extract_dem_tile(tiny_dem, e_sw, n_sw)
    assert raster is not None

    e_offset = e_sw - DEM_ORIGIN_E  # = TILE_SIDE_USFT
    for i in range(TILE_SIDE_USFT):
        expected_inches = int(round((e_offset + i) * 12))
        # All northing rows for this easting column should have the same value.
        assert np.all(raster[i, :] == expected_inches), (
            f"raster[{i}, :] expected {expected_inches}, got {raster[i, 0]}"
        )


def test_extract_height_conversion_to_inches(tiny_dem):
    """When extract_dem_tile converts heights, each value should equal usft * 12."""
    # Use a uniform DEM for simplicity.
    dem = np.full((1000, 1000), 10.0, dtype=np.float32)  # 10 usft everywhere
    e_sw = DEM_ORIGIN_E
    n_sw = DEM_ORIGIN_N - TILE_SIDE_USFT
    raster = extract_dem_tile(dem, e_sw, n_sw)
    assert raster is not None
    assert np.all(raster == 120)  # 10 usft * 12 = 120 inches


def test_extract_partial_tile_zero_padded():
    """When a tile extends beyond the DEM edge, out-of-DEM pixels should be 0."""
    # Tiny DEM: just 100 rows and 100 cols, all value 5 usft.
    dem = np.full((100, 100), 5.0, dtype=np.float32)
    # Tile whose SW corner aligns with south/west edge of DEM extent.
    e_sw = (DEM_ORIGIN_E // TILE_SIDE_USFT) * TILE_SIDE_USFT
    n_sw = ((DEM_ORIGIN_N - 100) // TILE_SIDE_USFT) * TILE_SIDE_USFT
    raster = extract_dem_tile(dem, e_sw, n_sw)
    assert raster is not None
    assert raster.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)
    # Some pixels are in the DEM (value 60 = 5*12), others are 0.
    assert np.any(raster == 60)
    assert np.any(raster == 0)


def test_extract_clips_negative_heights():
    """When DEM has nodata values encoded as negatives, extract_dem_tile should clip to 0."""
    dem = np.full((1000, 1000), -9999.0, dtype=np.float32)
    e_sw = DEM_ORIGIN_E
    n_sw = DEM_ORIGIN_N - TILE_SIDE_USFT
    raster = extract_dem_tile(dem, e_sw, n_sw)
    assert raster is not None
    assert np.all(raster == 0)


# ---------------------------------------------------------------------------
# split_dem (integration)
# ---------------------------------------------------------------------------

def test_split_dem_writes_tif_and_json(tmp_path, tiny_dem):
    """When split_dem runs, each tile should produce a paired .tif and .json file."""
    dem_path = tmp_path / "dem.tif"
    tifffile.imwrite(str(dem_path), tiny_dem)

    tile_ids = split_dem(dem_path, tmp_path / "out")
    assert len(tile_ids) > 0
    for tid in tile_ids:
        assert (tmp_path / "out" / f"{tid}.tif").exists()
        assert (tmp_path / "out" / f"{tid}.json").exists()


def test_split_dem_json_metadata(tmp_path, tiny_dem):
    """When split_dem writes a tile, the JSON should contain correct offsets and tile_id."""
    dem_path = tmp_path / "dem.tif"
    tifffile.imwrite(str(dem_path), tiny_dem)

    tile_ids = split_dem(dem_path, tmp_path / "out")
    for tid in tile_ids:
        meta = json.loads((tmp_path / "out" / f"{tid}.json").read_text())
        assert meta["tile_id"] == tid
        assert meta["x_offset"] % TILE_SIDE_USFT == 0
        assert meta["y_offset"] % TILE_SIDE_USFT == 0
        assert meta["raster_file"] == f"{tid}.tif"
        assert meta["obstruction_ids"] == []


def test_split_dem_tile_ids_use_canonical_extension_format(tmp_path, tiny_dem):
    """When split_dem produces tile IDs, each should end with an underscore and two digits."""
    dem_path = tmp_path / "dem.tif"
    tifffile.imwrite(str(dem_path), tiny_dem)

    tile_ids = split_dem(dem_path, tmp_path / "out")
    for tid in tile_ids:
        parts = tid.rsplit("_", 1)
        assert len(parts) == 2, f"tile_id {tid!r} missing underscore"
        assert len(parts[1]) == 2, f"extension {parts[1]!r} is not 2 digits"
        assert parts[1].isdigit(), f"extension {parts[1]!r} is not numeric"


def test_split_dem_raster_shape_and_dtype(tmp_path, tiny_dem):
    """When split_dem writes a tile, the .tif raster should be (500, 500) uint16."""
    dem_path = tmp_path / "dem.tif"
    tifffile.imwrite(str(dem_path), tiny_dem)

    tile_ids = split_dem(dem_path, tmp_path / "out")
    for tid in tile_ids:
        raster = tifffile.imread(str(tmp_path / "out" / f"{tid}.tif"))
        assert raster.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)
        assert raster.dtype == np.uint16


def test_split_dem_tile_sw_corners_match_json_offsets(tmp_path, tiny_dem):
    """When split_dem writes tiles, the JSON x/y_offset should equal the tile's SW corner."""
    dem_path = tmp_path / "dem.tif"
    tifffile.imwrite(str(dem_path), tiny_dem)

    tile_ids = split_dem(dem_path, tmp_path / "out")
    for tid in tile_ids:
        meta = json.loads((tmp_path / "out" / f"{tid}.json").read_text())
        # Reconstruct SW corner from tile_id using the canonical grid helpers.
        from los_analyzer.preprocessing.tile_id import file_id_to_offset, TILE_SIDE_USFT as T
        file_id, xi_str, yi_str = tid.rsplit("_", 1)[0], tid[-2], tid[-1]
        origin = file_id_to_offset(file_id)
        expected_x = origin[0] + int(xi_str) * T
        expected_y = origin[1] + int(yi_str) * T
        assert meta["x_offset"] == expected_x
        assert meta["y_offset"] == expected_y
