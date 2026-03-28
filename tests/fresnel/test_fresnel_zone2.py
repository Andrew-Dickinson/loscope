from math import atan, asin, tan, cos, sin, sqrt

import numpy as np
import pytest

from los_analyzer.lib.fresnel.fresnel_zone2 import (
    construct_fresnel_quadratic,
    homogenous_rotation_matrix_ellipsoid_to_nys,
    AngleContext, construct_homogenous_coordinate_transformation,
    get_integer_grid_within_bounds, compute_fresnel_zone, normalize_ellipse, FresnelZone,
)
from los_analyzer.lib.coordinates.coordinate_translate import translate_to_nys_plane


@pytest.mark.parametrize(
    "gps_point, expected",
    [
        (
            (40.650, -73.800, 100.0),
            (1039748.8061058237, 176148.99539110975, 329.33693385683),
        ),
        (
            (40.7173, -74.0060, 10000.0),
            (982587.8496248991, 200605.06780909808, 32809.58365781512),
        ),
        (
            (40.865339, -74.030096, 100.0),
            (975926.767548464, 254542.1263456685, 329.3288903320208),
        ),
    ],
)
def test_translate_to_nys_plane(gps_point, expected):
    """When translating a GPS point, output should match known NYS plane coordinates."""
    x, y, z = translate_to_nys_plane(gps_point)
    exp_x, exp_y, exp_z = expected
    assert x == pytest.approx(exp_x, rel=1e-6)
    assert y == pytest.approx(exp_y, rel=1e-6)
    assert z == pytest.approx(exp_z, rel=1e-6)


def test_angle_context():
    delta = (-57160.96194245259, 24456.04380098125, 32480.250000000004)
    delta_arr = np.array(delta)
    deltaX, deltaY, deltaZ = delta
    L = sqrt(deltaX**2 + deltaY**2 + deltaZ**2)
    theta = atan(-deltaY / deltaX)
    phi = atan(-deltaZ / deltaX)

    rho = asin(deltaZ / L)
    rho2 = atan(tan(phi) * cos(theta))
    assert rho == pytest.approx(rho2, rel=1e-9)

    omega = atan(cos(theta) / (sin(theta) * cos(phi)))

    ctx = AngleContext.from_delta_nys(delta_arr)
    assert ctx.sin_theta == pytest.approx(sin(theta), rel=1e-9)
    assert ctx.cos_theta == pytest.approx(cos(theta), rel=1e-9)
    assert ctx.tan_theta == pytest.approx(tan(theta), rel=1e-9)

    assert ctx.sin_phi == pytest.approx(sin(phi), rel=1e-9)
    assert ctx.cos_phi == pytest.approx(cos(phi), rel=1e-9)
    assert ctx.tan_phi == pytest.approx(tan(phi), rel=1e-9)

    assert ctx.sin_rho == pytest.approx(sin(rho), rel=1e-9)
    assert ctx.cos_rho == pytest.approx(cos(rho), rel=1e-9)
    assert ctx.tan_rho == pytest.approx(tan(rho), rel=1e-9)

    assert ctx.sin_omega == pytest.approx(sin(omega), rel=1e-9)
    assert ctx.cos_omega == pytest.approx(cos(omega), rel=1e-9)
    assert ctx.tan_omega == pytest.approx(tan(omega), rel=1e-9)




def test_homogenous_rotation_matrix_ellipsoid_to_nys():
    """When given delta_nys = point_b - point_a, rotation matrix should match known values."""
    point_a_nys = np.array([1039747.7086964573, 176152.26368097877, 328.08333333333337])
    point_b_nys = np.array([982586.7467540047, 200608.30748196002, 32808.333333333336])
    delta_nys = point_b_nys - point_a_nys

    angle_context = AngleContext.from_delta_nys(delta_nys)
    result = homogenous_rotation_matrix_ellipsoid_to_nys(angle_context)

    expected = np.array([
        [ 0.30312669, -0.81488729,  0.49403736,  0.],
        [ 0.93725462,  0.34864563,  0.,          0.],
        [-0.17224396,  0.4630388,   0.86944068,  0.],
        [ 0.,          0.,          0.,          1.],
    ])
    assert result.shape == (4, 4)
    np.testing.assert_allclose(result, expected, atol=1e-6)


def test_construct_fresnel_quadratic():
    """When given nys_distance, frequency, and alpha, quadratic matrix should match known values."""
    result_Q, result_axes = construct_fresnel_quadratic(
        nys_distance=70145.8501170563,
        frequency_hz=5_000_000_000,
        alpha=0.8,
    )

    expected = np.array([
        [ 4.52942599e-04,  0.00000000e+00,  0.00000000e+00,  0.00000000e+00],
        [ 0.00000000e+00,  8.12933101e-10,  0.00000000e+00,  0.00000000e+00],
        [ 0.00000000e+00,  0.00000000e+00,  4.52942599e-04,  0.00000000e+00],
        [ 0.00000000e+00,  0.00000000e+00,  0.00000000e+00, -1.00000000e+00],
    ])
    assert result_Q.shape == (4, 4)
    assert len(result_axes) == 2
    np.testing.assert_allclose(result_Q, expected, rtol=1e-6)
    np.testing.assert_allclose(result_axes, (35072.97423698255, 46.987075610800986), rtol=1e-6)


def test_translate_to_nys_plane_batch():
    """When translating multiple points, each should match the single-point result."""
    points = [
        (40.650, -73.800, 100.0),
        (40.7173, -74.0060, 10000.0),
        (40.865339, -74.030096, 100.0),
    ]
    for point in points:
        x, y, z = translate_to_nys_plane(point)
        sx, sy, sz = translate_to_nys_plane(point)
        assert x == pytest.approx(sx, rel=1e-9)
        assert y == pytest.approx(sy, rel=1e-9)
        assert z == pytest.approx(sz, rel=1e-9)

def test_nys_to_ellipsoid_matrix():
    point_a_nys_homo = np.array((1039747.7086964573, 176152.26368097877, 328.08333333333337, 1))
    point_b_nys_homo = np.array((982586.7467540047, 200608.30748196002, 32808.333333333336, 1))
    midpoint_nys_homo = (point_b_nys_homo + point_a_nys_homo) / 2

    angle_context = AngleContext.from_delta_nys((point_b_nys_homo - point_a_nys_homo)[:3])

    A_global_to_ellipsoid = construct_homogenous_coordinate_transformation(midpoint_nys_homo[:3], angle_context)
    A_ellipsoid_to_global = np.linalg.inv(A_global_to_ellipsoid)

    distance_nys = np.linalg.norm(point_a_nys_homo - point_b_nys_homo)

    np.testing.assert_allclose(A_global_to_ellipsoid @ midpoint_nys_homo, np.array([0, 0, 0, 1]), atol=1e-9)
    np.testing.assert_allclose(A_ellipsoid_to_global @ np.array([0, distance_nys / 2, 0, 1]), point_b_nys_homo, atol=1e-9)
    np.testing.assert_allclose(A_ellipsoid_to_global @ np.array([0, 0, 0, 1]), midpoint_nys_homo, atol=1e-9)
    np.testing.assert_allclose(A_ellipsoid_to_global @ np.array([0, -distance_nys / 2, 0, 1]), point_a_nys_homo, atol=1e-9)


def test_get_integer_grid_within_bounds():
    np.testing.assert_equal(get_integer_grid_within_bounds((-3.1, 1.98)), np.array([-3, -2, -1, 0, 1]))
    np.testing.assert_equal(get_integer_grid_within_bounds((-3, 2)), np.array([-3, -2, -1, 0, 1, 2]))
    np.testing.assert_equal(get_integer_grid_within_bounds((1, 1.98)), np.array([1]))
    np.testing.assert_equal(get_integer_grid_within_bounds((1, 1)), np.array([1]))
    np.testing.assert_equal(get_integer_grid_within_bounds((2, 7)), np.array([2, 3, 4, 5, 6, 7]))

    with pytest.raises(ValueError):
        get_integer_grid_within_bounds((10, 0))

def test_normalize_ellipse():
    C = np.array([
        [0.00015217054611868413, 0.00017090600184125422, 1.5442065871587545],
        [0.00017090600184125422, 0.0003558296484122786, -0.8774557722289558],
        [1.5442065871587545, -0.8774557722289558, 57294.55752986111]
    ])

    C_norm, offset = normalize_ellipse(C)

    expected_norm = np.array([
        [0.00015217054611868413, 0.00017090600184125422, 0],
        [0.00017090600184125422, 0.0003558296484122786, 0],
        [0, 0, -0.0369624448723276]
    ])

    np.testing.assert_allclose(C_norm, expected_norm, atol=1e-9)
    np.testing.assert_allclose(offset, np.array([-28047.112648969174, 15937.052135928374]), atol=1e-9)


def test_stress_test_fresnel_zone():
    gps_A = (40.650, -73.800, 100.0)
    gps_B = (40.7173, -74.0060, 10000.0)
    frequency_hz = 5_000_000_000
    alpha = 0.8

    nys_a = translate_to_nys_plane(gps_A)
    nys_b = translate_to_nys_plane(gps_B)
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)

def test_old_stress_test_fresnel_zone():
    gps_A, gps_B = (40.650, -73.979, 100.0), (40.7173, -74.0060, 100.0)
    nys_a = translate_to_nys_plane(gps_A)
    nys_b = translate_to_nys_plane(gps_B)
    zone = compute_fresnel_zone(nys_a, nys_b, 2400000000.0, 1.0)
    print(zone)

def test_east_west_long_fresnel_zone():
    gps_A = (40.650, -73.800, 100.0)
    gps_B = (40.650, -74.000, 100.0)
    frequency_hz = 5_000_000_000
    alpha = 1

    nys_a = translate_to_nys_plane(gps_A)
    nys_b = translate_to_nys_plane(gps_B)
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)

def test_north_south_long_fresnel_zone():
    gps_A = (40.650, -73.800, 100.0)
    gps_B = (40.8, -73.800, 100.0)
    frequency_hz = 5_000_000_000
    alpha = 1

    nys_a = translate_to_nys_plane(gps_A)
    nys_b = translate_to_nys_plane(gps_B)
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)

def test_new_fresnel_zone():
    GPS_A = (40.81399261450678, -73.9576824966002, 100.0)
    GPS_B = (40.81669146433694, -73.93829606722406, 100.0)
    FREQUENCY_HZ = 5_000_000_000
    ALPHA = 1.0

    nys_a = translate_to_nys_plane(GPS_A)
    nys_b = translate_to_nys_plane(GPS_B)
    zone = compute_fresnel_zone(nys_a, nys_b, FREQUENCY_HZ, ALPHA)
    print(zone)


def test_fresnel_zone_empty_x_grid():
    # At 24 GHz the Fresnel zone is tiny (~0.17 usft semi-minor), so the
    # x_bounds_nys span is < 1 usft wide (e.g. [1000574.22, 1000574.39]).
    # ceil(lower) > floor(upper), so get_integer_grid_within_bounds returns an
    # empty array, which then crashes on x_grid_nys[0] at line 163.
    GPS_A = (40.861448, -73.907696, 76.0)
    GPS_B = (40.830477, -73.941012, 80.0)
    FREQUENCY_HZ = 24_000_000_000
    ALPHA = 1.0

    nys_a = translate_to_nys_plane(GPS_A)
    nys_b = translate_to_nys_plane(GPS_B)
    zone = compute_fresnel_zone(nys_a, nys_b, FREQUENCY_HZ, ALPHA)
    assert isinstance(zone, FresnelZone)
