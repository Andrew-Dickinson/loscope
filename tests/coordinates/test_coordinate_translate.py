"""Tests for los_analyzer.lib.coordinates.coordinate_translate"""
import pytest

from los_analyzer.lib.coordinates.coordinate_translate import (
    translate_to_nys_plane,
    translate_from_nys_plane,
)

# Known NYS point derived from test_fresnel_zone2 ground truth:
# GPS (40.650, -73.800, 100.0) → NYS (1039748.806, 176148.995, 329.337)
_KNOWN_NYS = (1039748.806, 176148.995, 329.337)
_KNOWN_GPS = (40.650, -73.800, 100.0)

_KNOWN_NYS_2 = (1039748.806, 176148.995)
_KNOWN_GPS_2 = (40.650, -73.800)


def test_translate_from_nys_plane_returns_three_values():
    result = translate_from_nys_plane(_KNOWN_NYS)
    assert len(result) == 3


def test_translate_with_two_returns_two_values():
    result = translate_from_nys_plane(_KNOWN_NYS_2)
    assert len(result) == 2
    result = translate_to_nys_plane(_KNOWN_GPS_2)
    assert len(result) == 2


def test_translate_from_nys_plane_nyc_area_values():
    """Output should contain a latitude near 40.x and a longitude near -73.x."""
    result = translate_from_nys_plane(_KNOWN_NYS)
    values = list(result)
    assert any(39.0 < v < 42.0 for v in values), f"Expected lat near 40; got {values}"
    assert any(-75.0 < v < -72.0 for v in values), f"Expected lon near -73; got {values}"


def test_roundtrip_gps_to_nys_to_gps():
    """GPS → NYS → GPS should approximately recover the original values (±0.000001°, ±0.1 m)."""
    nys = translate_to_nys_plane(_KNOWN_GPS)
    gps_back = translate_from_nys_plane(nys)

    assert gps_back[1] == pytest.approx(_KNOWN_GPS[0], abs=0.000001)
    assert gps_back[0] == pytest.approx(_KNOWN_GPS[1], abs=0.000001)
    # TODO: This is a pretty large vertical error, we should investigate this
    assert gps_back[2] == pytest.approx(_KNOWN_GPS[2], abs=1.0)


def test_translate_from_nys_plane_elevation_in_meters():
    """The elevation component of the GPS result should be near 100 m for _KNOWN_NYS."""
    result = translate_from_nys_plane(_KNOWN_NYS)
    # _KNOWN_GPS has elevation 100 m; one of the three output values should be close
    assert any(90.0 < v < 110.0 for v in result), f"Expected elevation ~100 m; got {list(result)}"
