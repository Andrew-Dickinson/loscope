from math import atan, asin, tan, cos, sin, sqrt

import numpy as np
import pytest

from los_analyzer.fresnel.fresnel_zone2 import (
    construct_fresnel_quadratic,
    homogenous_rotation_matrix_ellipsoid_to_nys,
    translate_to_nys_plane, AngleContext, construct_homogenous_coordinate_transformation,
    get_integer_grid_within_bounds, compute_fresnel_zone, normalize_ellipse,
)


@pytest.mark.parametrize(
    "gps_point, expected",
    [
        (
            (40.650, -73.800, 100.0),
            (1039747.7086964573, 176152.26368097877, 328.08333333333337),
        ),
        (
            (40.7173, -74.0060, 10000.0),
            (982586.7467540047, 200608.30748196002, 32808.333333333336),
        ),
        (
            (40.865339, -74.030096, 100.0),
            (975925.6527077489, 254545.4059772802, 328.08333333333337),
        ),
    ],
)
def test_translate_to_nys_plane(gps_point, expected):
    """When translating a GPS point, output should match known NYS plane coordinates."""
    result = translate_to_nys_plane([gps_point])
    assert len(result) == 1
    x, y, z = result[0]
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
    """When translating multiple points at once, each should match the single-point result."""
    points = [
        (40.650, -73.800, 100.0),
        (40.7173, -74.0060, 10000.0),
        (40.865339, -74.030096, 100.0),
    ]
    batch = translate_to_nys_plane(points)
    for point, result in zip(points, batch):
        single = translate_to_nys_plane([point])
        x, y, z = result
        sx, sy, sz = single[0]
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

    nys_a, nys_b = translate_to_nys_plane([gps_A, gps_B])
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)

def test_old_stress_test_fresnel_zone():
    gps_A, gps_B = (40.650, -73.979, 100.0), (40.7173, -74.0060, 100.0)
    nys_a, nys_b = translate_to_nys_plane([gps_A, gps_B])
    zone = compute_fresnel_zone(nys_a, nys_b, 2400000000.0, 1.0)
    print(zone)

def test_east_west_long_fresnel_zone():
    gps_A = (40.650, -73.800, 100.0)
    gps_B = (40.650, -74.000, 100.0)
    frequency_hz = 5_000_000_000
    alpha = 1

    nys_a, nys_b = translate_to_nys_plane([gps_A, gps_B])
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)

def test_north_south_long_fresnel_zone():
    gps_A = (40.650, -73.800, 100.0)
    gps_B = (40.8, -73.800, 100.0)
    frequency_hz = 5_000_000_000
    alpha = 1

    nys_a, nys_b = translate_to_nys_plane([gps_A, gps_B])
    zone = compute_fresnel_zone(nys_a, nys_b, frequency_hz, alpha)
    print(zone)