"""Tests for los_analyzer.lib.providers.dob_db_dao — DOBDBDAO"""
import sqlite3
from pathlib import Path

import pytest

from los_analyzer.lib.providers.dob_db_dao import DOBDBDAO

_VALID_POLYGON_WKT = "POLYGON ((0 0, 0 10, 10 10, 10 0, 0 0))"
_VALID_BIN = "1234567"


def _create_db(db_path: Path, rows: list[tuple[str, str]]) -> None:
    """Create a minimal building_footprints SQLite database."""
    con = sqlite3.connect(str(db_path))
    con.execute("CREATE TABLE building_footprints (bin TEXT, the_geom TEXT)")
    con.executemany("INSERT INTO building_footprints VALUES (?, ?)", rows)
    con.commit()
    con.close()


@pytest.fixture
def db_path(tmp_path) -> Path:
    path = tmp_path / "test.db"
    _create_db(path, [
        (_VALID_BIN, _VALID_POLYGON_WKT),
        ("9999999", "POLYGON ((100 100, 100 110, 110 110, 110 100, 100 100))"),
    ])
    return path


@pytest.fixture
def dao(db_path) -> DOBDBDAO:
    return DOBDBDAO(db_path)


# ---------------------------------------------------------------------------
# Successful lookup
# ---------------------------------------------------------------------------

def test_returns_geometry_for_known_bin(dao):
    geom = dao.fetch_building_footprint_geometry(_VALID_BIN)
    assert geom is not None


def test_returned_geometry_is_not_empty(dao):
    geom = dao.fetch_building_footprint_geometry(_VALID_BIN)
    assert not geom.is_empty


def test_returned_geometry_is_a_polygon(dao):
    from shapely.geometry import Polygon
    geom = dao.fetch_building_footprint_geometry(_VALID_BIN)
    assert isinstance(geom, Polygon)


def test_second_bin_also_resolves(dao):
    geom = dao.fetch_building_footprint_geometry("9999999")
    assert geom is not None
    assert not geom.is_empty


# ---------------------------------------------------------------------------
# Missing / bad data
# ---------------------------------------------------------------------------

def test_raises_value_error_for_missing_bin(dao):
    with pytest.raises(ValueError, match="not found"):
        dao.fetch_building_footprint_geometry("0000000")


def test_raises_value_error_for_null_geom(tmp_path):
    path = tmp_path / "null.db"
    _create_db(path, [("0000001", None)])
    d = DOBDBDAO(path)
    with pytest.raises(ValueError):
        d.fetch_building_footprint_geometry("0000001")


def test_raises_value_error_for_invalid_wkt(tmp_path):
    path = tmp_path / "bad.db"
    _create_db(path, [("0000002", "NOT VALID WKT AT ALL")])
    d = DOBDBDAO(path)
    with pytest.raises(ValueError, match="Could not parse"):
        d.fetch_building_footprint_geometry("0000002")


def test_raises_value_error_for_empty_geometry(tmp_path):
    path = tmp_path / "empty.db"
    _create_db(path, [("0000003", "GEOMETRYCOLLECTION EMPTY")])
    d = DOBDBDAO(path)
    with pytest.raises(ValueError, match="[Ee]mpty"):
        d.fetch_building_footprint_geometry("0000003")
