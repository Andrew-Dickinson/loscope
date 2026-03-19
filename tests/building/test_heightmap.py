"""Tests for src.los_analyzer.building.heightmap"""
from __future__ import annotations

import sqlite3
from pathlib import Path
from unittest.mock import patch

import numpy as np
import pytest
import tifffile
from shapely.geometry import box as shapely_box

from los_analyzer.building.heightmap import build_building_heightmap, fetch_building_geometry


# ---------------------------------------------------------------------------
# Helpers
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


def _make_tile(tmp_path: Path, tile_id: str, x_offset: int, y_offset: int, fill: int = 1000) -> None:
    """Write a 500×500 uint16 tile TIF with a uniform fill value."""
    raster = np.full((500, 500), fill, dtype=np.uint16)
    tifffile.imwrite(str(tmp_path / f"{tile_id}.tif"), raster)


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
# fetch_building_geometry
# ---------------------------------------------------------------------------

def test_fetch_building_geometry_not_found(tmp_path):
    """When the BIN is not in the DB, fetch_building_geometry should raise ValueError."""
    db = _make_db(tmp_path, "9999999", _POLY_WKT)
    with pytest.raises(ValueError, match="not found"):
        fetch_building_geometry(_BIN, db)


def test_fetch_building_geometry_empty_geom(tmp_path):
    """When the geometry field is empty, fetch_building_geometry should raise ValueError."""
    db = _make_db(tmp_path, _BIN, "")
    # The row exists but the_geom is an empty string → falsy
    with pytest.raises(ValueError, match=_BIN):
        fetch_building_geometry(_BIN, db)


def test_fetch_building_geometry_returns_polygon(tmp_path):
    """When a valid WKT polygon is stored, fetch_building_geometry should return a shapely geometry."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    geom = fetch_building_geometry(_BIN, db)
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
        build_building_heightmap(_BIN, db, tile_dir)


def test_build_building_heightmap_no_tiles(tmp_path):
    """When _intersecting_tile_ids returns an empty list, raise ValueError."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    with patch(
        "los_analyzer.building.heightmap._intersecting_tile_ids",
        return_value=[],
    ):
        with pytest.raises(ValueError, match="No preprocessed tiles"):
            build_building_heightmap(_BIN, db, tile_dir)


# ---------------------------------------------------------------------------
# build_building_heightmap — blit correctness
# ---------------------------------------------------------------------------

def test_build_building_heightmap_blits_correct_region(tmp_path):
    """Heights from the overlapping tile region should appear in the heightmap."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()

    # Fill the tile with a known value; the building polygon covers
    # tile pixels [100:200, 100:200] (since x_sw=912600 = tile_x+100).
    fill_value = 720  # 60 ft in inches
    raster = np.zeros((500, 500), dtype=np.uint16)
    raster[100:200, 100:200] = fill_value
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    heightmap, mask, poly, x_sw, y_sw, tile_ids = build_building_heightmap(
        _BIN, db, tile_dir
    )

    assert heightmap.shape == (100, 100)
    assert x_sw == 912600
    assert y_sw == 117600
    # All pixels inside the mask should have the fill value
    assert (heightmap[mask == 255] == fill_value).all()
    assert _TILE_ID in tile_ids


def test_build_building_heightmap_zeros_outside_mask(tmp_path):
    """Pixels outside the footprint mask should be zeroed out."""
    # Use a triangular polygon so some grid pixels are outside
    triangle_wkt = (
        "POLYGON ((912600 117600, 912700 117600, 912700 117700, 912600 117600))"
    )
    db = _make_db(tmp_path, _BIN, triangle_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    raster = np.full((500, 500), 500, dtype=np.uint16)
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    heightmap, mask, poly, x_sw, y_sw, _ = build_building_heightmap(
        _BIN, db, tile_dir
    )

    # Pixels outside the mask must be 0
    outside = mask == 0
    assert (heightmap[outside] == 0).all()


def test_build_building_heightmap_mask_uses_buffer(tmp_path):
    """The mask rasterizer should include boundary-crossing pixels (0.5 ft buffer).

    The polygon is tiny (0.3×0.3 ft) so the pixel centre at offset (0.5, 0.5)
    is outside the polygon itself, but inside the 0.5 ft buffer — so it should
    be included in the mask.
    """
    tiny_poly_wkt = (
        "POLYGON ((912600 117600, 912600.3 117600, "
        "912600.3 117600.3, 912600 117600.3, 912600 117600))"
    )
    db = _make_db(tmp_path, _BIN, tiny_poly_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    raster = np.full((500, 500), 600, dtype=np.uint16)
    tifffile.imwrite(str(tile_dir / f"{_TILE_ID}.tif"), raster)

    heightmap, mask, poly, x_sw, y_sw, _ = build_building_heightmap(
        _BIN, db, tile_dir
    )

    # The single pixel whose centre is at (x_sw+0.5, y_sw+0.5) = (912600.5, 117600.5)
    # is 0.2 ft east of the polygon east edge (912600.3), well within 0.5 ft buffer.
    assert mask[0, 0] == 255


def test_build_building_heightmap_partial_tile_overlap(tmp_path):
    """When the building spans two tiles, both tiles should be blitted."""
    # Building straddles tiles "912117_00" (912500–913000) and "912117_10" (913000–913500)
    cross_poly_wkt = (
        "POLYGON ((912990 117600, 913010 117600, 913010 117700, 912990 117700, 912990 117600))"
    )
    db = _make_db(tmp_path, _BIN, cross_poly_wkt)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()

    # Tile "912117_00": x_offset=912500, fill pixels [490:500, 100:200] with 720
    raster_00 = np.zeros((500, 500), dtype=np.uint16)
    raster_00[490:500, 100:200] = 720  # last 10 columns that overlap building
    tifffile.imwrite(str(tile_dir / "912117_00.tif"), raster_00)

    # Tile "912117_10": x_offset=913000, fill pixels [0:10, 100:200] with 840
    raster_10 = np.zeros((500, 500), dtype=np.uint16)
    raster_10[0:10, 100:200] = 840  # first 10 columns that overlap building
    tifffile.imwrite(str(tile_dir / "912117_10.tif"), raster_10)

    heightmap, mask, poly, x_sw, y_sw, tile_ids = build_building_heightmap(
        _BIN, db, tile_dir
    )

    assert x_sw == 912990
    assert y_sw == 117600

    # West half (local x 0:10) should come from tile "912117_00"
    west_vals = heightmap[0:10, :][mask[0:10, :] == 255]
    if west_vals.size > 0:
        assert (west_vals == 720).all()

    # East half (local x 10:20) should come from tile "912117_10"
    east_vals = heightmap[10:20, :][mask[10:20, :] == 255]
    if east_vals.size > 0:
        assert (east_vals == 840).all()

    assert "912117_00" in tile_ids
    assert "912117_10" in tile_ids


def test_build_building_heightmap_skips_missing_tile(tmp_path):
    """When a tile file is missing on disk, the function should proceed with zeros."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    # Do NOT create the tile file

    # Should not raise; the heightmap is all zeros (tile was skipped)
    heightmap, mask, poly, x_sw, y_sw, tile_ids = build_building_heightmap(
        _BIN, db, tile_dir
    )
    assert (heightmap == 0).all()
    assert tile_ids == []


def test_build_building_heightmap_returns_correct_sw_corner(tmp_path):
    """x_sw and y_sw should equal floor(minx) and floor(miny) of the polygon."""
    db = _make_db(tmp_path, _BIN, _POLY_WKT)
    tile_dir = tmp_path / "tiles"
    tile_dir.mkdir()
    _make_tile(tile_dir, _TILE_ID, _TILE_X, _TILE_Y)

    _, _, _, x_sw, y_sw, _ = build_building_heightmap(_BIN, db, tile_dir)
    assert x_sw == 912600
    assert y_sw == 117600
