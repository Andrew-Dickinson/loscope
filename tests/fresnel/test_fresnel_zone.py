"""Tests for fresnel_zone.compute_fresnel_zone."""

import numpy as np
import pytest

from los_analyzer.lib.fresnel.fresnel_zone import (
    SPEED_OF_LIGHT_M_S,
    USFT_PER_METER,
    FresnelZone,
    compute_fresnel_zone,
)
from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT

# Synthetic link: two NYC rooftop coordinates ~500 m apart, at 100 m elevation
POINT_A = (40.7128, -74.0060, 100.0)   # approx NYC
POINT_B = (40.7173, -74.0060, 100.0)   # ~500 m north
FREQ_24 = 2.4e9   # 2.4 GHz
FREQ_58 = 5.8e9   # 5.8 GHz


@pytest.fixture(scope="module")
def zone_24() -> FresnelZone:
    return compute_fresnel_zone(POINT_A, POINT_B, FREQ_24)


@pytest.fixture(scope="module")
def zone_24_alpha06() -> FresnelZone:
    return compute_fresnel_zone(POINT_A, POINT_B, FREQ_24, alpha=0.6)


@pytest.fixture(scope="module")
def zone_58() -> FresnelZone:
    return compute_fresnel_zone(POINT_A, POINT_B, FREQ_58)


def test_output_shape_matches_grid_bounds(zone_24: FresnelZone) -> None:
    """When computing a zone, top/bottom/mask should all share the same (W, H) shape."""
    W, H = zone_24.top.shape
    assert W > 0 and H > 0
    assert zone_24.bottom.shape == (W, H)
    assert zone_24.mask.shape == (W, H)


def test_offsets_are_integers(zone_24: FresnelZone) -> None:
    """When offsets are returned, they should be plain Python ints."""
    assert isinstance(zone_24.x_offset, int)
    assert isinstance(zone_24.y_offset, int)


def test_buffer_expands_by_one_tile(zone_24: FresnelZone) -> None:
    """When building grid bounds, offsets should be at least TILE_SIDE_USFT beyond the fresnel extent."""
    # The fresnel cells that are inside the mask determine the inner extent.
    xs, ys = np.where(zone_24.mask)
    # Convert grid indices back to NYS coordinates
    x_coords = xs + zone_24.x_offset
    y_coords = ys + zone_24.y_offset

    inner_x_min = int(x_coords.min())
    inner_x_max = int(x_coords.max())
    inner_y_min = int(y_coords.min())
    inner_y_max = int(y_coords.max())

    W, H = zone_24.mask.shape
    x_end = zone_24.x_offset + W
    y_end = zone_24.y_offset + H

    assert zone_24.x_offset <= inner_x_min - TILE_SIDE_USFT
    assert x_end >= inner_x_max + TILE_SIDE_USFT
    assert zone_24.y_offset <= inner_y_min - TILE_SIDE_USFT
    assert y_end >= inner_y_max + TILE_SIDE_USFT


def test_mask_true_at_los_midpoint(zone_24: FresnelZone) -> None:
    """When querying the grid cell at the horizontal LOS midpoint, mask should be 1."""
    import pyproj

    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    t = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)

    xA, yA, _ = t.transform(*POINT_A)
    xB, yB, _ = t.transform(*POINT_B)

    mid_x = int(round((xA + xB) / 2))
    mid_y = int(round((yA + yB) / 2))

    i = mid_x - zone_24.x_offset
    j = mid_y - zone_24.y_offset

    assert zone_24.mask[i, j] == 1


def test_top_bottom_symmetric_at_los_footprint(zone_24: FresnelZone) -> None:
    """When h=0 (on the LOS footprint), top - z_los should equal z_los - bottom."""
    import pyproj

    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    t_tf = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)

    xA, yA, zA = t_tf.transform(*POINT_A)
    xB, yB, zB = t_tf.transform(*POINT_B)

    mid_x = int(round((xA + xB) / 2))
    mid_y = int(round((yA + yB) / 2))

    i = mid_x - zone_24.x_offset
    j = mid_y - zone_24.y_offset

    z_mid = (zA + zB) / 2
    assert np.isclose(zone_24.top[i, j] - z_mid, z_mid - zone_24.bottom[i, j], rtol=1e-6)


def test_alpha_scales_radius(zone_24: FresnelZone, zone_24_alpha06: FresnelZone) -> None:
    """When alpha=0.6, the resulting mask should be a subset of alpha=1.0 mask."""
    z1 = zone_24
    z06 = zone_24_alpha06

    # Align on the common offset range
    assert z1.x_offset <= z06.x_offset
    assert z1.y_offset <= z06.y_offset

    xi_start = z06.x_offset - z1.x_offset
    yi_start = z06.y_offset - z1.y_offset
    W06, H06 = z06.mask.shape
    mask1_crop = z1.mask[xi_start:xi_start + W06, yi_start:yi_start + H06]

    # Every cell in alpha=0.6 mask must also be in alpha=1.0 mask
    assert np.all((z06.mask == 0) | (mask1_crop == 1))


def test_higher_frequency_smaller_zone(zone_24: FresnelZone, zone_58: FresnelZone) -> None:
    """When frequency increases, the Fresnel zone should shrink."""
    assert zone_58.mask.sum() < zone_24.mask.sum()


def test_fresnel_radius_at_midpoint_matches_formula(zone_24: FresnelZone) -> None:
    """When computing radius at midpoint, it should match alpha*sqrt(lambda*L/4)."""
    import pyproj

    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    ecef_crs = pyproj.CRS.from_epsg(4978)
    t_ecef = pyproj.Transformer.from_crs(gps_crs, ecef_crs, always_xy=False)

    A_ecef = np.array(t_ecef.transform(*POINT_A))
    B_ecef = np.array(t_ecef.transform(*POINT_B))
    L_m = np.linalg.norm(B_ecef - A_ecef)
    wavelength_m = SPEED_OF_LIGHT_M_S / FREQ_24

    expected_r_m = np.sqrt(wavelength_m * L_m / 4)
    expected_r_usft = expected_r_m * USFT_PER_METER

    # At the midpoint grid cell, top = z_los + r_usft so r_usft = top - z_los
    import pyproj as _pyproj
    nys_crs = _pyproj.CRS.from_string("EPSG:6539+6360")
    t_nys = _pyproj.Transformer.from_crs(
        _pyproj.CRS.from_string("EPSG:4326+5773"), nys_crs, always_xy=False
    )
    xA, yA, zA = t_nys.transform(*POINT_A)
    xB, yB, zB = t_nys.transform(*POINT_B)

    mid_x = int(round((xA + xB) / 2))
    mid_y = int(round((yA + yB) / 2))
    i = mid_x - zone_24.x_offset
    j = mid_y - zone_24.y_offset

    z_mid = (zA + zB) / 2
    actual_r_usft = zone_24.top[i, j] - z_mid

    assert np.isclose(actual_r_usft, expected_r_usft, rtol=0.01)


def test_top_bottom_nan_outside_mask(zone_24: FresnelZone) -> None:
    """When mask==0, top and bottom should be NaN."""
    outside = zone_24.mask == 0
    assert np.all(np.isnan(zone_24.top[outside]))
    assert np.all(np.isnan(zone_24.bottom[outside]))


def test_horizontal_link_symmetric_top_bottom(zone_24: FresnelZone) -> None:
    """When the link is flat (same altitude), top+bottom should equal 2*z_los everywhere inside mask."""
    import pyproj

    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    t_nys = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)

    xA, yA, zA = t_nys.transform(*POINT_A)
    xB, yB, zB = t_nys.transform(*POINT_B)

    # Both endpoints are at the same altitude (100m), so z_los is constant
    assert np.isclose(zA, zB, rtol=1e-4), "Fixture points must have equal altitudes"

    inside = zone_24.mask == 1
    midsum = zone_24.top[inside] + zone_24.bottom[inside]
    z_los_val = zA  # constant since zA==zB
    assert np.allclose(midsum, 2 * z_los_val, rtol=1e-6)
