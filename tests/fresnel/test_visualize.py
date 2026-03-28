"""Tests for los_analyzer.lib.fresnel.visualize"""
import numpy as np

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.fresnel.visualize import create_zone_obj

# tile "235_00": e0=1000000, n0=235000, extends to e=1000500, n=235500
_TILE_ID = "235_00"


def _make_zone(
    H: int = 100,
    maxW: int = 200,
    top_val: int = 1440,    # 120 ft
    bottom_val: int = 1200, # 100 ft
    x_base_offset: int = 1000050,
    y_base_offset: int = 235100,
) -> FresnelZone:
    """Build a simple FresnelZone that overlaps tile "235_00"."""
    return FresnelZone(
        top=np.full((H, maxW), top_val, dtype=np.uint16),
        bottom=np.full((H, maxW), bottom_val, dtype=np.uint16),
        widths=np.full(H, maxW, dtype=np.uint32),
        offsets=np.zeros(H, dtype=np.uint32),
        x_base_offset=x_base_offset,
        y_base_offset=y_base_offset,
    )


def test_returns_bytesio_for_overlapping_zone():
    zone = _make_zone()
    assert create_zone_obj(zone, _TILE_ID) is not None


def test_obj_contains_comment_header():
    zone = _make_zone()
    content = create_zone_obj(zone, _TILE_ID).read().decode()
    assert "# Fresnel zone volume mesh" in content
    assert "1 unit = 1 US survey foot" in content


def test_obj_contains_object_name():
    zone = _make_zone()
    content = create_zone_obj(zone, _TILE_ID).read().decode()
    assert f"o zone_{_TILE_ID.replace('-', '_')}" in content


def test_obj_contains_vertices_and_faces():
    zone = _make_zone()
    content = create_zone_obj(zone, _TILE_ID).read().decode()
    assert "v " in content
    assert "f " in content


def test_vertex_z_values_match_top_bottom():
    """Vertices should appear at z=120.000 (top) and z=100.000 (bottom)."""
    zone = _make_zone(top_val=1440, bottom_val=1200)
    content = create_zone_obj(zone, _TILE_ID).read().decode()
    assert "120.000" in content
    assert "100.000" in content


def test_returns_none_when_zone_does_not_overlap_tile():
    """A zone entirely outside the tile's northing range should return None."""
    # y_base_offset=500000 puts the zone far north of tile "235_00" (max n=235500)
    zone = _make_zone(y_base_offset=500000)
    assert create_zone_obj(zone, _TILE_ID) is None


def test_returns_none_when_zone_easting_misses_tile():
    """A zone with x offset far east of the tile should return None."""
    # x_base_offset=1100000 + maxW=10 puts zone at [1100000, 1100010), tile ends at 1000500
    zone = _make_zone(maxW=10, x_base_offset=1100000)
    assert create_zone_obj(zone, _TILE_ID) is None


def test_face_indices_are_positive_integers():
    """All face indices in the OBJ should be positive integers."""
    zone = _make_zone(H=5, maxW=10)
    content = create_zone_obj(zone, _TILE_ID).read().decode()
    for line in content.splitlines():
        if line.startswith("f "):
            indices = line[2:].split()
            assert all(int(idx) > 0 for idx in indices)
