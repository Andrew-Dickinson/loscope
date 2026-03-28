"""Tests for los_analyzer.lib.building.heightmap"""
from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import Optional
from unittest.mock import patch

import numpy as np
import pytest
import tifffile

from los_analyzer.lib.building.heightmap import (
    RooftopHeightMap,
    build_building_heightmap,
    filter_heightmap_outliers,
)
from los_analyzer.lib.preprocessing.tile import TileData
from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.lib.providers.dob_db_dao import DOBDBDAO
from los_analyzer.lib.providers.tile_provider import TileProvider


# ---------------------------------------------------------------------------
# Test-only helpers
# ---------------------------------------------------------------------------

def _make_db(tmp_path: Path, bin_id: str, geom_wkt: str | None) -> Path:
    """Create a minimal SQLite DB with the building_footprints table."""
    db = tmp_path / "test.db"
    con = sqlite3.connect(str(db))
    con.execute("CREATE TABLE building_footprints (bin TEXT, the_geom TEXT)")
    if geom_wkt is not None:
        con.execute("INSERT INTO building_footprints VALUES (?, ?)", (bin_id, geom_wkt))
    con.commit()
    con.close()
    return db


class FsTileProvider(TileProvider):
    """Read .tif from a directory; return an empty tile when file is missing."""

    def __init__(self, tile_dir: Path):
        self._tile_dir = Path(tile_dir)

    def get_tile(self, tile_id: str) -> Optional[TileData]:
        from los_analyzer.lib.preprocessing.tile_id import tile_id_to_offset
        x_offset, y_offset = tile_id_to_offset(tile_id)
        tif = self._tile_dir / f"{tile_id}.tif"
        if not tif.exists():
            return TileData(
                tile_id=tile_id,
                x_offset=x_offset,
                y_offset=y_offset,
                raster=np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16),
            )
        raster = tifffile.imread(str(tif))
        return TileData(tile_id=tile_id, x_offset=x_offset, y_offset=y_offset, raster=raster)


# A valid polygon centred in tile "912117_00" (x_offset=912500, y_offset=117500).
# Uses a 100×100 ft region entirely within the tile.
_BIN = "1234567"
_POLY_WKT = (
    "POLYGON ((912600 117600, 912700 117600, 912700 117700, 912600 117700, 912600 117600))"
)
_TILE_ID = "912117_00"
_TILE_X = 912500
_TILE_Y = 117500


# ---------------------------------------------------------------------------
# DOBDBDAO.fetch_building_footprint_geometry
# ---------------------------------------------------------------------------

def test_fetch_building_geometry_not_found(tmp_path):
    """When the BIN is not in the DB, fetch_building_footprint_geometry should raise ValueError."""
    db = _make_db(tmp_path, "9999999", _POLY_WKT)
    dao = DOBDBDAO(db)
    with pytest.raises(ValueError, match="not found"):
        dao.fetch_building_footprint_geometry(_BIN)


def test_fetch_building_geometry_empty_geom(tmp_path):
    """When the geometry field is empty, fetch_building_footprint_geometry should raise ValueError."""
    db = _make_db(tmp_path, _BIN, "")
    dao = DOBDBDAO(db)
    with pytest.raises(ValueError):
        dao.fetch_building_footprint_geometry(_BIN)


def test_fetch_building_geometry_returns_polygon(tmp_path):
    """When a valid WKT polygon is stored, fetch_building_footprint_geometry returns a shapely geometry."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    dao = DOBDBDAO(db)
    geom = dao.fetch_building_footprint_geometry(_BIN)
    assert not geom.is_empty
    minx, miny, maxx, maxy = geom.bounds
    assert minx == pytest.approx(912600.0)
    assert miny == pytest.approx(117600.0)


# ---------------------------------------------------------------------------
# build_building_heightmap — error cases
# ---------------------------------------------------------------------------

def test_build_building_heightmap_bin_not_found(tmp_path):
    """When the BIN is missing from the DB, build_building_heightmap should raise ValueError."""
    db = _make_db(tmp_path, "9999999", _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    with pytest.raises(ValueError):
        build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))


def test_build_building_heightmap_no_tiles(tmp_path):
    """When _intersecting_tile_ids returns an empty list, raise ValueError."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    with patch(
        "los_analyzer.lib.building.heightmap._intersecting_tile_ids",
        return_value=[],
    ):
        with pytest.raises(ValueError, match="No preprocessed tiles"):
            build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))


# ---------------------------------------------------------------------------
# build_building_heightmap — blit correctness
# ---------------------------------------------------------------------------

def test_build_building_heightmap_blits_correct_region(tmp_path):
    """Heights from the overlapping tile region should appear in the heightmap."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()

    fill_value = 720  # 60 ft in inches
    raster = np.zeros((500, 500), dtype=np.uint16)
    raster[100:200, 100:200] = fill_value
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))

    assert isinstance(result, RooftopHeightMap)
    assert result.heightmap.shape == (100, 100)
    assert result.x_sw == 912600
    assert result.y_sw == 117600
    assert (result.heightmap[result.mask == 255] == fill_value).all()


def test_build_building_heightmap_zeros_outside_mask(tmp_path):
    """Pixels outside the footprint mask should be zeroed out."""
    triangle_wkt = (
        "POLYGON ((912600 117600, 912700 117600, 912700 117700, 912600 117600))"
    )
    db = _make_db(tmp_path, _BIN, triangle_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    raster = np.full((500, 500), 500, dtype=np.uint16)
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))

    outside = result.mask == 0
    assert (result.heightmap[outside] == 0).all()


def test_build_building_heightmap_mask_uses_buffer(tmp_path):
    """The mask rasterizer should include boundary-crossing pixels (0.5 ft buffer)."""
    tiny_poly_wkt = (
        "POLYGON ((912600 117600, 912600.3 117600, "
        "912600.3 117600.3, 912600 117600.3, 912600 117600))"
    )
    db = _make_db(tmp_path, _BIN, tiny_poly_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    raster = np.full((500, 500), 600, dtype=np.uint16)
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))

    assert result.mask[0, 0] == 255


def test_build_building_heightmap_partial_tile_overlap(tmp_path):
    """When the building spans two tiles, both tiles should be blitted."""
    cross_poly_wkt = (
        "POLYGON ((912990 117600, 913010 117600, 913010 117700, 912990 117700, 912990 117600))"
    )
    db = _make_db(tmp_path, _BIN, cross_poly_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()

    raster_00 = np.zeros((500, 500), dtype=np.uint16)
    raster_00[490:500, 100:200] = 720
    tifffile.imwrite(str(tile_dir / "912117_00.tif"), raster_00)

    raster_10 = np.zeros((500, 500), dtype=np.uint16)
    raster_10[0:10, 100:200] = 840
    tifffile.imwrite(str(tile_dir / "912117_10.tif"), raster_10)

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))

    assert result.x_sw == 912990
    assert result.y_sw == 117600

    west_vals = result.heightmap[0:10, :][result.mask[0:10, :] == 255]
    if west_vals.size > 0:
        assert (west_vals == 720).all()

    east_vals = result.heightmap[10:20, :][result.mask[10:20, :] == 255]
    if east_vals.size > 0:
        assert (east_vals == 840).all()


def test_build_building_heightmap_skips_missing_tile(tmp_path):
    """When a tile file is missing on disk, the heightmap should be all zeros."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    # Do NOT create the tile file — FsTileProvider returns zeros for missing tiles

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))
    assert (result.heightmap == 0).all()


def test_build_building_heightmap_returns_correct_sw_corner(tmp_path):
    """x_sw and y_sw should equal floor(minx) and floor(miny) of the polygon."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    raster = np.full((500, 500), 1000, dtype=np.uint16)
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    result = build_building_heightmap(_BIN, DOBDBDAO(db), FsTileProvider(tile_dir))
    assert result.x_sw == 912600
    assert result.y_sw == 117600


# ---------------------------------------------------------------------------
# filter_heightmap_outliers
# ---------------------------------------------------------------------------

def _make_uniform_heightmap(W, H, value, mask_value=255):
    heightmap = np.full((W, H), value, dtype=np.uint16)
    mask = np.full((W, H), mask_value, dtype=np.uint8)
    return heightmap, mask


def test_filter_uniform_heightmap_unchanged():
    """A uniform heightmap has zero std dev — no pixels should be replaced."""
    heightmap, mask = _make_uniform_heightmap(20, 20, 600)
    result = filter_heightmap_outliers(heightmap, mask)
    np.testing.assert_array_equal(result, heightmap)


def test_filter_output_dtype_is_uint16():
    """filter_heightmap_outliers should always return a uint16 array."""
    heightmap, mask = _make_uniform_heightmap(10, 10, 500)
    result = filter_heightmap_outliers(heightmap, mask)
    assert result.dtype == np.uint16


def test_filter_does_not_modify_input():
    """filter_heightmap_outliers should return a copy and leave the input unchanged."""
    heightmap, mask = _make_uniform_heightmap(10, 10, 500)
    original = heightmap.copy()
    filter_heightmap_outliers(heightmap, mask)
    np.testing.assert_array_equal(heightmap, original)


def test_filter_replaces_spike_with_local_median():
    """A single spike pixel surrounded by uniform neighbours should be corrected."""
    W, H = 15, 15
    base = 600
    spike = 60000

    heightmap = np.full((W, H), base, dtype=np.uint16)
    mask = np.full((W, H), 255, dtype=np.uint8)

    cx, cy = 7, 7
    heightmap[cx, cy] = spike

    result = filter_heightmap_outliers(heightmap, mask, radius=3.0, threshold_sigma=3.0)

    assert result[cx, cy] != spike
    assert result[cx - 1, cy] == base
    assert result[cx + 1, cy] == base


def test_filter_outside_mask_pixels_unchanged():
    """Pixels outside the mask should be left untouched regardless of their value."""
    W, H = 15, 15
    heightmap = np.full((W, H), 600, dtype=np.uint16)
    mask = np.zeros((W, H), dtype=np.uint8)
    mask[7, 7] = 255
    heightmap[0, 0] = 60000

    result = filter_heightmap_outliers(heightmap, mask, radius=3.0)
    assert result[0, 0] == 60000


def test_filter_non_outlier_pixels_not_replaced():
    """With a very high sigma threshold, no pixel should be replaced."""
    W, H = 15, 15
    heightmap = np.zeros((W, H), dtype=np.uint16)
    for i in range(W):
        heightmap[i, :] = 600 + i * 2
    mask = np.full((W, H), 255, dtype=np.uint8)

    result = filter_heightmap_outliers(heightmap, mask, radius=3.0, threshold_sigma=100.0)
    np.testing.assert_array_equal(result, heightmap)


def test_filter_empty_mask_returns_copy():
    """When the mask is all zeros, the function should return an unchanged copy."""
    heightmap = np.full((10, 10), 500, dtype=np.uint16)
    mask = np.zeros((10, 10), dtype=np.uint8)
    result = filter_heightmap_outliers(heightmap, mask)
    np.testing.assert_array_equal(result, heightmap)
