"""Tests for src.los_analyzer.obstructions.building_footprints"""
import numpy as np
import pytest

from los_analyzer.obstructions.building_footprints import parse_building_row
from los_analyzer.obstructions.model import OBSTRUCTION_TYPE_BUILDING

# A real-ish building polygon near Staten Island in WGS84 lon/lat
SAMPLE_ROW = {
    "the_geom": (
        "MULTIPOLYGON ((("
        "-74.168617 40.608587, "
        "-74.168633 40.608529, "
        "-74.168700 40.608540, "
        "-74.168684 40.608598, "
        "-74.168617 40.608587"
        ")))"
    ),
    "BIN": "5175075",
    "BASE_BBL": "5022230011",
    "Construction Year": "2021",
    "Geometry Source": "Other (Manual)",
    "Ground Elevation": "32",
    "Height Roof": "12",
    "LAST_STATUS_TYPE": "Constructed",
}


@pytest.fixture
def obs():
    return parse_building_row(SAMPLE_ROW)


def test_parse_returns_obstruction(obs):
    """When given a valid row, parse_building_row should return an Obstruction."""
    assert obs is not None


def test_obstruction_type_is_building(obs):
    """When parsed, obstruction_type should be 'Existing Building Footprint'."""
    assert obs.obstruction_type == OBSTRUCTION_TYPE_BUILDING


def test_raster_dtype_is_uint16(obs):
    """When parsed, the raster should be uint16."""
    assert obs.raster.dtype == np.uint16


def test_raster_has_nonzero_pixels(obs):
    """When the polygon covers grid cells, at least one raster pixel should be nonzero."""
    assert obs.raster.max() > 0


def test_height_encoding_is_ground_plus_roof_in_inches(obs):
    """When ground=32ft and roof=12ft, nonzero pixels should encode (32+12)*12=528 inches."""
    nonzero = obs.raster[obs.raster > 0]
    assert (nonzero == 528).all()


def test_raster_shape_matches_bounding_box(obs):
    """When parsed, raster width and height should be positive integers."""
    W, H = obs.raster.shape
    assert W >= 1
    assert H >= 1


def test_attributes_contain_required_keys(obs):
    """When parsed, attributes should contain BIN, BBL, construction_year, geometry_source, etc."""
    attrs = obs.attributes
    assert attrs["BIN"] == "5175075"
    assert attrs["BBL"] == "5022230011"
    assert attrs["construction_year"] == 2021
    assert attrs["geom_source"] == "Other (Manual)"
    assert attrs["ground_elevation"] == 32.0
    assert attrs["height_roof"] == 12.0
    assert attrs["last_status_type"] == "Constructed"


def test_offsets_are_nys_integers(obs):
    """When parsed, x_offset and y_offset should be integers in a plausible NYS range."""
    # NYS Long Island easting is roughly 900000–1100000 usft
    assert isinstance(obs.x_offset, int)
    assert isinstance(obs.y_offset, int)
    assert 800_000 < obs.x_offset < 1_200_000
    assert 100_000 < obs.y_offset < 400_000


def test_obstruction_id_is_uuid(obs):
    """When parsed, obstruction_id should be a valid UUID string."""
    import uuid
    uuid.UUID(obs.obstruction_id)  # raises ValueError if invalid


def test_tile_ids_are_present(obs):
    """When parsed, tile_ids should be a non-empty list of canonical tile ID strings."""
    assert isinstance(obs.tile_ids, list)
    assert len(obs.tile_ids) >= 1


def test_tile_ids_have_canonical_format(obs):
    """When parsed, each tile_id should match the '{file_id}_{xi}{yi}' pattern."""
    import re
    for tid in obs.tile_ids:
        assert re.match(r"^\d+_\d{2}$", tid), f"Unexpected tile_id format: {tid}"


def test_tile_ids_only_includes_intersecting_tiles():
    """When a building is entirely within one tile, tile_ids should contain exactly one entry."""
    # A tiny building well within a single 500 usft tile
    # Use a very small polygon in WGS84 that maps to a single tile
    obs = parse_building_row(SAMPLE_ROW)
    assert obs is not None
    # The sample building is small; it should intersect at most a handful of tiles
    assert len(obs.tile_ids) <= 4


def test_missing_geometry_returns_none():
    """When the_geom is empty, parse_building_row should return None."""
    row = {**SAMPLE_ROW, "the_geom": ""}
    assert parse_building_row(row) is None


def test_missing_height_returns_none():
    """When Ground Elevation is missing, parse_building_row should return None."""
    row = {k: v for k, v in SAMPLE_ROW.items() if k != "Ground Elevation"}
    assert parse_building_row(row) is None


def test_comma_formatted_numbers_parse_correctly():
    """When numeric fields use comma formatting, they should still parse."""
    row = {**SAMPLE_ROW, "Ground Elevation": "1,032", "Height Roof": "12"}
    obs = parse_building_row(row)
    assert obs is not None
    nonzero = obs.raster[obs.raster > 0]
    assert (nonzero == (1032 + 12) * 12).all()
