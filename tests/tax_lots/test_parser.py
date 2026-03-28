"""Tests for los_analyzer.lib.tax_lots.parser"""
from __future__ import annotations

import csv
import json
from pathlib import Path

import pytest

from los_analyzer.lib.tax_lots.parser import (
    BOROUGH_NAMES,
    _round_coords,
    parse_row,
    parse_csv,
    write_json_files,
)

# A small polygon inside Manhattan in NYS EPSG:6539 coords
_POLY_WKT = (
    "POLYGON ((982600 200600, 982700 200600, "
    "982700 200700, 982600 200700, 982600 200600))"
)

_VALID_ROW = {
    "the_geom": _POLY_WKT,
    "boro": "1",
    "block": "100",
    "lot": "1",
    "bbl": "1001001",
}


# ---------------------------------------------------------------------------
# parse_row — success path
# ---------------------------------------------------------------------------

def test_parse_row_returns_dict_for_valid_row():
    result = parse_row(_VALID_ROW)
    assert result is not None
    assert isinstance(result, dict)


def test_parse_row_contains_expected_keys():
    result = parse_row(_VALID_ROW)
    for key in ("borough", "block", "lot", "bbl", "geometry"):
        assert key in result


def test_parse_row_geometry_has_crs():
    result = parse_row(_VALID_ROW)
    assert result["geometry"]["crs"] == "EPSG:6539"


def test_parse_row_geometry_type_is_polygon():
    result = parse_row(_VALID_ROW)
    assert result["geometry"]["type"] == "Polygon"


def test_parse_row_borough_value_stored():
    result = parse_row(_VALID_ROW)
    assert result["borough"] == "1"


def test_parse_row_block_and_lot_stored():
    result = parse_row(_VALID_ROW)
    assert result["block"] == "100"
    assert result["lot"] == "1"


def test_parse_row_all_five_boroughs():
    """All five NYC borough codes should be accepted."""
    for boro_num in BOROUGH_NAMES:
        row = {**_VALID_ROW, "boro": boro_num}
        assert parse_row(row) is not None, f"boro {boro_num} should be valid"


def test_parse_row_coordinates_are_integers():
    """Returned coordinates should be rounded to integers."""
    wkt_frac = (
        "POLYGON ((982600.7 200600.2, 982700.3 200600.8, "
        "982700.9 200700.1, 982600.1 200700.6, 982600.7 200600.2))"
    )
    result = parse_row({**_VALID_ROW, "the_geom": wkt_frac})
    for x, y in result["geometry"]["coordinates"][0]:
        assert x == int(x)
        assert y == int(y)


# ---------------------------------------------------------------------------
# parse_row — error cases
# ---------------------------------------------------------------------------

def test_parse_row_returns_none_when_geom_empty():
    assert parse_row({**_VALID_ROW, "the_geom": ""}) is None


def test_parse_row_returns_none_when_boro_empty():
    assert parse_row({**_VALID_ROW, "boro": ""}) is None


def test_parse_row_returns_none_when_block_empty():
    assert parse_row({**_VALID_ROW, "block": ""}) is None


def test_parse_row_returns_none_when_lot_empty():
    assert parse_row({**_VALID_ROW, "lot": ""}) is None


def test_parse_row_returns_none_for_unknown_borough():
    assert parse_row({**_VALID_ROW, "boro": "9"}) is None


def test_parse_row_returns_none_for_invalid_wkt():
    assert parse_row({**_VALID_ROW, "the_geom": "NOT VALID WKT"}) is None


def test_parse_row_returns_none_for_empty_geometry():
    assert parse_row({**_VALID_ROW, "the_geom": "POLYGON EMPTY"}) is None


# ---------------------------------------------------------------------------
# _round_coords
# ---------------------------------------------------------------------------

def test_round_coords_polygon_rounds_to_int():
    geom = {
        "type": "Polygon",
        "coordinates": [[(1.4, 2.7), (3.5, 4.1), (5.9, 6.0), (1.4, 2.7)]],
    }
    result = _round_coords(geom)
    assert result["type"] == "Polygon"
    for x, y in result["coordinates"][0]:
        assert x == int(x)
        assert y == int(y)


def test_round_coords_polygon_values_correct():
    geom = {
        "type": "Polygon",
        "coordinates": [[(1.4, 2.7), (3.5, 4.1), (1.4, 2.7)]],
    }
    result = _round_coords(geom)
    assert result["coordinates"][0][0] == (1, 3)
    assert result["coordinates"][0][1] == (4, 4)


def test_round_coords_multipolygon():
    geom = {
        "type": "MultiPolygon",
        "coordinates": [
            [[(10.4, 20.6), (30.5, 40.1), (50.9, 60.3), (10.4, 20.6)]],
        ],
    }
    result = _round_coords(geom)
    assert result["type"] == "MultiPolygon"
    ring = result["coordinates"][0][0]
    for x, y in ring:
        assert x == int(x)
        assert y == int(y)


# ---------------------------------------------------------------------------
# parse_csv
# ---------------------------------------------------------------------------

def _write_csv(path: Path, rows: list[dict]) -> None:
    fields = ["the_geom", "boro", "block", "lot", "bbl"]
    with open(path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def test_parse_csv_returns_nonempty_for_valid_rows(tmp_path):
    csv_path = tmp_path / "lots.csv"
    _write_csv(csv_path, [_VALID_ROW])
    result = parse_csv(csv_path)
    total = sum(
        len(lots)
        for blocks in result.values()
        for lots in blocks.values()
    )
    assert total == 1


def test_parse_csv_skips_invalid_rows(tmp_path):
    csv_path = tmp_path / "lots.csv"
    rows = [
        {**_VALID_ROW, "boro": "9"},  # invalid borough → skipped
        _VALID_ROW,
    ]
    _write_csv(csv_path, rows)
    result = parse_csv(csv_path)
    total = sum(
        len(lots)
        for blocks in result.values()
        for lots in blocks.values()
    )
    assert total == 1


def test_parse_csv_groups_by_borough_then_block(tmp_path):
    csv_path = tmp_path / "lots.csv"
    row2 = {**_VALID_ROW, "block": "200", "lot": "2"}
    _write_csv(csv_path, [_VALID_ROW, row2])
    result = parse_csv(csv_path)
    boro_data = result["1"]
    assert "100" in boro_data
    assert "200" in boro_data


def test_parse_csv_warns_when_rows_skipped(tmp_path):
    csv_path = tmp_path / "lots.csv"
    rows = [{**_VALID_ROW, "boro": "9"}, _VALID_ROW]
    _write_csv(csv_path, rows)
    import warnings
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        parse_csv(csv_path)
    assert any("Skipped" in str(warning.message) for warning in w)


# ---------------------------------------------------------------------------
# write_json_files
# ---------------------------------------------------------------------------

_SAMPLE_LOT = {
    "borough": "1",
    "block": "100",
    "lot": "1",
    "bbl": "1001001",
    "geometry": {"type": "Polygon", "crs": "EPSG:6539", "coordinates": []},
}


def test_write_json_files_creates_borough_directory(tmp_path):
    data = {"1": {"100": {"1": _SAMPLE_LOT}}}
    write_json_files(data, tmp_path)
    assert (tmp_path / "1").is_dir()


def test_write_json_files_returns_file_count(tmp_path):
    data = {"1": {"100": {"1": _SAMPLE_LOT}}}
    count = write_json_files(data, tmp_path)
    assert count == 1


def test_write_json_files_creates_one_file_per_block(tmp_path):
    lot2 = {**_SAMPLE_LOT, "block": "200", "lot": "2"}
    data = {"1": {"100": {"1": _SAMPLE_LOT}, "200": {"2": lot2}}}
    count = write_json_files(data, tmp_path)
    assert count == 2
    assert (tmp_path / "1" / "100.json").exists()
    assert (tmp_path / "1" / "200.json").exists()


def test_write_json_files_content_is_valid_json(tmp_path):
    data = {"1": {"100": {"1": _SAMPLE_LOT}}}
    write_json_files(data, tmp_path)
    content = (tmp_path / "1" / "100.json").read_text()
    parsed = json.loads(content)
    assert "1" in parsed
    assert parsed["1"]["bbl"] == "1001001"


def test_write_json_files_multiple_boroughs(tmp_path):
    lot_bk = {**_SAMPLE_LOT, "borough": "3"}
    data = {
        "1": {"100": {"1": _SAMPLE_LOT}},
        "3": {"500": {"1": lot_bk}},
    }
    count = write_json_files(data, tmp_path)
    assert count == 2
    assert (tmp_path / "1").is_dir()
    assert (tmp_path / "3").is_dir()
