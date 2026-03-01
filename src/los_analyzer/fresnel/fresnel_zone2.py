from __future__ import annotations

from dataclasses import dataclass
import pyproj
import math
import numpy as np

from math import sqrt, ceil, floor

SPEED_OF_LIGHT_M_S = 299_792_458
USFT_PER_METER = 1 / 0.3048006096
EARTH_RADIUS_METERS = 6_369_160 # (approx) In NYC
EARTH_RADIUS_USFT = EARTH_RADIUS_METERS * USFT_PER_METER

class AngleContext:
    # To avoid imprecision and domain issues associated with going back and forth between angles
    tan_theta: float
    sin_theta: float
    cos_theta: float

    tan_phi: float
    sin_phi: float
    cos_phi: float

    tan_rho: float
    sin_rho: float
    cos_rho: float

    tan_omega: float
    sin_omega: float
    cos_omega: float

    @staticmethod
    def from_delta_nys(delta_nys: np.array) -> AngleContext:
        def sin_of_atan(x):
            return x / sqrt(1 + x ** 2)

        def cos_of_atan(x):
            return 1 / sqrt(1 + x ** 2)

        ctx = AngleContext()
        ctx.tan_theta = -delta_nys[1] / delta_nys[0]
        ctx.sin_theta = sin_of_atan(ctx.tan_theta)
        ctx.cos_theta = cos_of_atan(ctx.tan_theta)

        ctx.tan_phi = -delta_nys[2] / delta_nys[0]
        ctx.sin_phi = sin_of_atan(ctx.tan_phi)
        ctx.cos_phi = cos_of_atan(ctx.tan_phi)

        ctx.tan_rho = ctx.tan_phi * ctx.cos_theta
        ctx.sin_rho = sin_of_atan(ctx.tan_rho)
        ctx.cos_rho = cos_of_atan(ctx.tan_rho)

        ctx.tan_omega = ctx.cos_theta / (ctx.sin_theta * ctx.cos_phi)
        ctx.sin_omega = sin_of_atan(ctx.tan_omega)
        ctx.cos_omega = cos_of_atan(ctx.tan_omega)

        return ctx



@dataclass
class FresnelZone:
    top: np.ndarray       # float64, shape (W, H), usft; NaN outside mask
    bottom: np.ndarray    # float64, shape (W, H), usft; NaN outside mask
    mask: np.ndarray      # uint8,   shape (W, H), 1=present 0=absent
    x_offset: int         # min easting  (west edge)  of grid in NYS usft
    y_offset: int         # min northing (south edge) of grid in NYS usft


def compute_fresnel_zone(
    point_a_nys_in: tuple[float, float, float],
    point_b_nys_in: tuple[float, float, float],
    frequency_hz: float,
    alpha: float = 1.0,
) -> FresnelZone:
    # Initial coordinate transformation into NYS, angle computations, etc
    point_a_nys, point_b_nys = np.array(point_a_nys_in), np.array(point_b_nys_in)
    delta_nys = point_b_nys - point_a_nys
    distance_nys = np.linalg.norm(delta_nys)
    midpoint_nys = (point_a_nys + point_b_nys) / 2

    angle_context = AngleContext.from_delta_nys(delta_nys)

    R_around_Z_for_correction_func = np.array([
        [angle_context.sin_theta, angle_context.cos_theta],
        [-angle_context.cos_theta, angle_context.sin_theta]
    ])

    # Build ellipsoid, coordinate transforms
    Q_ellipsoid, (semi_major, semi_minor) = construct_fresnel_quadratic(distance_nys, frequency_hz, alpha)
    major_axis = 2 * semi_major
    A_nys_to_ellipsoid = construct_homogenous_coordinate_transformation(midpoint_nys, angle_context)
    Q_nys = np.linalg.matrix_transpose(A_nys_to_ellipsoid) @ Q_ellipsoid @ A_nys_to_ellipsoid

    # Establish bounds for the zone, in the NYS Y axis
    max_t = np.linalg.norm(np.array([semi_minor, semi_major]) * np.array([angle_context.sin_omega, angle_context.cos_omega]))
    t_bounds = (-max_t, max_t)
    t_vals = get_integer_grid_within_bounds(t_bounds)

    for i, t in enumerate(t_vals):
        # Plane representing y=t in the NYS coordinate system
        offset_homogenous = np.append(midpoint_nys, 1)
        offset_homogenous[1] += t
        E_slice_plane_nys = np.hstack((
            np.array([
                [1, 0],
                [0, 0],
                [0, 1],
                [0, 0]
            ]),
            offset_homogenous.reshape(-1, 1)
        ))

        # Compute a matrix representation of a conic section representing the intersection, and normalize it by
        # applying a transformation to move its center to (0, 0) in its own reference frame
        C_ellipse_conic_section = np.linalg.matrix_transpose(E_slice_plane_nys) @ Q_nys @ E_slice_plane_nys
        C_ellipse_conic_section_norm, ellipse_offset_xz = normalize_ellipse(C_ellipse_conic_section)

        # Sample the ellipse at points which slot neatly into a height map rasterized over integer NYS coordinates
        ellipse_x_bounds = compute_ellipse_x_bounds(C_ellipse_conic_section_norm)
        ellipse_nys_x_offset = ellipse_offset_xz[0] + midpoint_nys[0]
        x_bounds_nys = ellipse_nys_x_offset + np.array(ellipse_x_bounds)
        x_grid_nys = get_integer_grid_within_bounds(x_bounds_nys)
        x_grid_ellipse = x_grid_nys - ellipse_nys_x_offset

        lower_z_points_ellipse, upper_z_points_ellipse = sample_conic_at_x_grid(C_ellipse_conic_section_norm, x_grid_ellipse)
        ellipse_nys_z_offset = ellipse_offset_xz[1] + midpoint_nys[2]
        lower_z_points_nys = lower_z_points_ellipse + ellipse_nys_z_offset
        upper_z_points_nys = upper_z_points_ellipse + ellipse_nys_z_offset

        # Correct for distortion due to the conic projection of the NYS Plane that we just worked in
        #
        # Strictly speaking, the below implementation is not correct, since it assumes constant distortion radially from
        # the LOS line. In reality, there is increased distortion further from the line, but since the radius of the
        # zone <<< length of the line, we can neglect this effect to make the computation SIGNIFICANTLY easier. It's
        # also an approximation even on the centerline, since it assumes a spherical earth. However, we expect these
        # errors to be < 0.1 usft so we don't care
        x_grid_relative_to_midpoint = x_grid_nys - midpoint_nys[0]
        xy_grid_coordinates_relative_to_midpoint = np.vstack((x_grid_relative_to_midpoint, np.full(x_grid_relative_to_midpoint.shape, t)))
        sample_point_grid_axial_distance_from_center = (R_around_Z_for_correction_func @ xy_grid_coordinates_relative_to_midpoint)[1]

        sample_point_grid_correction_factor = np.sqrt(EARTH_RADIUS_USFT**2 - sample_point_grid_axial_distance_from_center**2) - math.sqrt(EARTH_RADIUS_USFT**2 - (1 / 4)*major_axis**2)
        corrected_lower = lower_z_points_nys - sample_point_grid_correction_factor
        corrected_upper = upper_z_points_nys - sample_point_grid_correction_factor




def translate_to_nys_plane(gps_points: list[tuple[float, float, float]]) -> list[tuple[float, float, float]]:
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    gps_to_nys = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)
    return [gps_to_nys.transform(*point) for point in gps_points]

def homogenous_rotation_matrix_ellipsoid_to_nys(ctx: AngleContext):
    return np.array([
        [ctx.cos_phi * ctx.sin_theta * ctx.cos_rho, - ctx.cos_theta * ctx.cos_rho, ctx.sin_phi, 0],
        [ctx.sin_rho * ctx.sin_phi + ctx.cos_phi * ctx.cos_theta * ctx.cos_rho, ctx.sin_theta * ctx.cos_rho, 0, 0],
        [-ctx.sin_phi * ctx.cos_rho * ctx.sin_theta, ctx.sin_rho, ctx.cos_phi, 0],
        [0, 0, 0, 1],
    ])

def homogenous_translation_matrix_for_offset(offset: np.array):
    return np.array([
        [1, 0, 0, offset[0]],
        [0, 1, 0, offset[1]],
        [0, 0, 1, offset[2]],
        [0, 0, 0, 1]
    ])

def construct_fresnel_quadratic(nys_distance: float, frequency_hz: float, alpha: float) -> tuple[np.array, tuple[float, float]]:
    wavelength_meters = SPEED_OF_LIGHT_M_S / frequency_hz
    wavelength_usft = wavelength_meters * USFT_PER_METER

    semi_major = (nys_distance + wavelength_usft / 2) / 2
    semi_minor = (sqrt(wavelength_usft * nys_distance) / 2) * alpha

    return np.diag([
        1 / semi_minor ** 2,
        1 / semi_major ** 2,
        1 / semi_minor ** 2,
        -1
    ]), (semi_major, semi_minor)

def construct_homogenous_coordinate_transformation(midpoint_nys: np.array, angle_context: AngleContext):
    R_ellipsoid_to_nys = homogenous_rotation_matrix_ellipsoid_to_nys(angle_context)
    T_center_to_nys = homogenous_translation_matrix_for_offset(midpoint_nys)

    A_ellipsoid_to_nys = T_center_to_nys @ R_ellipsoid_to_nys
    A_nys_to_ellipsoid = np.linalg.inv(A_ellipsoid_to_nys)

    return A_nys_to_ellipsoid

def get_integer_grid_within_bounds(bounds: tuple[float, float]):
    if bounds[0] > bounds[1]:
        raise ValueError(f"Invalid bounds: {bounds}")

    # __BOTH__ BOUNDS ARE __INCLUSIVE__
    grid_bounds = np.array([ceil(bounds[0]), floor(bounds[1])])
    grid_width = grid_bounds[1] - grid_bounds[0]

    # +1 is to convert width to an inclusive upper bound
    return  np.linspace(grid_bounds[0], grid_bounds[1], grid_width + 1, dtype=np.int64)

def normalize_ellipse(C_conic_section: np.array) -> tuple[np.array, tuple[float, float]]:
    ellipse_offset_homogenous = np.linalg.inv(C_conic_section)[:, 2]
    ellipse_offset = tuple(ellipse_offset_homogenous[:2] / ellipse_offset_homogenous[2])

    B_ellipse_normalization = np.array([
        [1, 0, ellipse_offset[0]],
        [0, 1, ellipse_offset[1]],
        [0, 0, 1]
    ])

    # TODO: Maybe we don't need to do this whole matrix computation, we can use a formula for f in terms of a,b,c
    return np.linalg.matrix_transpose(B_ellipse_normalization) @ C_conic_section @ B_ellipse_normalization, ellipse_offset


def compute_ellipse_x_bounds(C_ellipse_conic_section_norm: np.array) -> tuple[float, float]:
    a = C_ellipse_conic_section_norm[0, 0]
    b = C_ellipse_conic_section_norm[1, 0]
    c = C_ellipse_conic_section_norm[1, 1]
    f = C_ellipse_conic_section_norm[2, 2]

    max_x_offset = sqrt(c * f / (b ** 2 - a * c))
    return (-max_x_offset, max_x_offset)


def sample_conic_at_x_grid(C_ellipse_conic_section_norm: np.array, x_grid: np.array) -> tuple(np.array, np.array):
    a = C_ellipse_conic_section_norm[0, 0]
    b = C_ellipse_conic_section_norm[1, 0]
    c = C_ellipse_conic_section_norm[1, 1]
    f = C_ellipse_conic_section_norm[2, 2]

    sqrt_term = np.sqrt(x_grid ** 2 * ((b ** 2 - a * c) / c ** 2) - (f / c))
    lin_term = (-b / c) * x_grid
    return lin_term - sqrt_term, lin_term + sqrt_term