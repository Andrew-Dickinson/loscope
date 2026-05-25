use crate::analysis::angle_context::AngleContext;
use crate::analysis::point_evaluation::PointEvaluationInput;
use crate::types::coords::NYSCoords2;
use crate::types::stairstep::{StairStepGrid, WincodeGridElem};
use derive_more::From;
use nalgebra::{Matrix3, Matrix4, SMatrix, Vector4};
use ndarray::{Array1, Array2};
use rocket::serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use wincode::{SchemaRead, SchemaWrite};

const OFFSET_BUFFER: f64 = 500.0;
const SPEED_OF_LIGHT_M_S: f64 = 299_792_458.0;
const USFT_PER_METER: f64 = 1.0 / 0.3048006096;
const EARTH_RADIUS_METERS: f64 = 6_369_160.0;
const EARTH_RADIUS_USFT: f64 = EARTH_RADIUS_METERS * USFT_PER_METER;

#[derive(Serialize, Deserialize, SchemaWrite, SchemaRead, From, Default, Copy, Clone)]
#[repr(C)]
pub struct FresnelZonePoint(u16, u16);

impl WincodeGridElem for FresnelZonePoint {
    type Wire = FresnelZonePoint;
    fn into_wire(self) -> FresnelZonePoint {
        self
    }
    fn from_wire(w: FresnelZonePoint) -> Self {
        w
    }
}

impl FresnelZonePoint {
    pub fn new(bottom: u16, top: u16) -> FresnelZonePoint {
        FresnelZonePoint(bottom, top)
    }
    pub fn bottom(&self) -> u16 {
        self.0
    }
    pub fn top(&self) -> u16 {
        self.1
    }
}

pub type FresnelZone = StairStepGrid<FresnelZonePoint>;

/// Build the ellipsoid quadratic form Q (4×4 diagonal) plus semi-axes.
fn construct_fresnel_quadratic(dist: f64, freq_hz: f64, alpha: f64) -> (Matrix4<f64>, f64, f64) {
    let wl_usft = (SPEED_OF_LIGHT_M_S / freq_hz) * USFT_PER_METER;
    let semi_major = (dist + wl_usft / 2.0) / 2.0;
    let semi_minor = (wl_usft * dist).sqrt() / 2.0 * alpha;
    let q = Matrix4::from_diagonal(&Vector4::new(
        1.0 / (semi_minor * semi_minor),
        1.0 / (semi_major * semi_major),
        1.0 / (semi_minor * semi_minor),
        -1.0,
    ));
    (q, semi_major, semi_minor)
}

/// 4×4 homogeneous rotation: ellipsoid frame → NYS frame.
fn rotation_ellipsoid_to_nys(ctx: &AngleContext) -> Matrix4<f64> {
    let AngleContext {
        sin_theta,
        cos_theta,
        sin_phi,
        cos_phi,
        sin_rho,
        cos_rho,
        ..
    } = *ctx;
    Matrix4::new(
        cos_phi * sin_theta * cos_rho,
        -cos_theta * cos_rho,
        sin_phi,
        0.0,
        sin_rho * sin_phi + cos_phi * cos_theta * cos_rho,
        sin_theta * cos_rho,
        0.0,
        0.0,
        -sin_phi * cos_rho * sin_theta,
        sin_rho,
        cos_phi,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    )
}

/// 4×4 homogeneous translation matrix.
fn translation_matrix(offset: (f64, f64, f64)) -> Matrix4<f64> {
    Matrix4::new(
        1.0, 0.0, 0.0, offset.0, 0.0, 1.0, 0.0, offset.1, 0.0, 0.0, 1.0, offset.2, 0.0, 0.0, 0.0,
        1.0,
    )
}

/// 4×4 homogeneous transform: NYS frame → ellipsoid frame (A_ellipsoid_to_nys inverted).
fn nys_to_ellipsoid_transform(mid: (f64, f64, f64), ctx: &AngleContext) -> Matrix4<f64> {
    let a_ell_to_nys = translation_matrix(mid) * rotation_ellipsoid_to_nys(ctx);
    a_ell_to_nys
        .try_inverse()
        .expect("rotation is always invertible")
}

/// Integer grid [ceil(lo), floor(hi)] inclusive.
fn integer_grid(lo: f64, hi: f64) -> RangeInclusive<i64> {
    assert!(lo <= hi);
    let start = lo.ceil() as i64;
    let end = hi.floor() as i64;
    start..=end
}

/// Translate the conic centre to the origin; return (C_norm, u, v).
fn normalize_ellipse(c: &Matrix3<f64>) -> Option<(Matrix3<f64>, f64, f64)> {
    let inv = c.try_inverse()?;
    let h = inv[(2, 2)];
    let u = inv[(0, 2)] / h;
    let v = inv[(1, 2)] / h;

    let a = c[(0, 0)];
    let b = c[(1, 0)];
    let cc = c[(1, 1)];
    let e = c[(2, 0)];
    let g = c[(2, 1)];
    let f = c[(2, 2)];

    let f_norm = u * (a * u + 2.0 * b * v + 2.0 * e) + v * (cc * v + 2.0 * g) + f;

    let c_norm = Matrix3::new(a, b, 0.0, b, cc, 0.0, 0.0, 0.0, f_norm);
    Some((c_norm, u, v))
}

/// Half-width in x of the normalised centred ellipse, or None if degenerate.
fn ellipse_half_width(c: &Matrix3<f64>) -> Option<f64> {
    let a = c[(0, 0)];
    let b = c[(1, 0)];
    let cc = c[(1, 1)];
    let f = c[(2, 2)];
    let disc = b * b - a * cc; // < 0 for an ellipse
    if disc >= 0.0 {
        return None;
    }
    let x2 = cc * f / disc;
    if x2 < 0.0 {
        return None;
    }
    Some(x2.sqrt())
}

/// Sample the centred ellipse conic at each x, returning (lower_z, upper_z).
fn sample_conic<I: Iterator<Item = f64>>(
    c: &Matrix3<f64>,
    x_vals: I,
) -> impl Iterator<Item = (f64, f64)> {
    let a = c[(0, 0)];
    let b = c[(1, 0)];
    let cc = c[(1, 1)];
    let f = c[(2, 2)];

    let k_lin = -b / cc;
    let k_sq = (b * b - a * cc) / (cc * cc);
    let k_const = -f / cc;

    x_vals.map(move |x| {
        let sq = (k_sq * x * x + k_const).max(0.0).sqrt();
        let lin = k_lin * x;
        (lin - sq, lin + sq)
    })
}

pub fn compute_fresnel_zone(
    point_evaluation_input: &PointEvaluationInput,
    alpha: f64,
) -> FresnelZone {
    let pa: (f64, f64, f64) = point_evaluation_input.point_a().into();
    let mut pb: (f64, f64, f64) = point_evaluation_input.point_b().into();
    let mut delta = (pb.0 - pa.0, pb.1 - pa.1, pb.2 - pa.2);

    // Avoid divide by zero case by shifting one endpoint by a few inches rather than panic-ing
    if delta.0 == 0.0 {
        pb.0 += 0.1;
        delta.0 = 0.1;
    }

    let dist = (delta.0 * delta.0 + delta.1 * delta.1 + delta.2 * delta.2).sqrt();
    let mid = (
        (pa.0 + pb.0) / 2.0,
        (pa.1 + pb.1) / 2.0,
        (pa.2 + pb.2) / 2.0,
    );

    let ctx = AngleContext::from_delta(delta);

    // Second row of R_around_Z: [-cos_theta, sin_theta] — used for curvature correction.
    let r1_dx = -ctx.cos_theta;
    let r1_dy = ctx.sin_theta;

    let (q_ellipsoid, semi_major, semi_minor) =
        construct_fresnel_quadratic(dist, *point_evaluation_input.frequency_hz(), alpha);
    let major_axis = 2.0 * semi_major;

    let a_nys_to_ell = nys_to_ellipsoid_transform(mid, &ctx);

    // Q expressed in NYS: Q_nys = A^T Q_ell A
    let q_nys = a_nys_to_ell.transpose() * q_ellipsoid * a_nys_to_ell;

    // Y grid
    let max_t =
        ((semi_minor * ctx.sin_omega).powi(2) + (semi_major * ctx.cos_omega).powi(2)).sqrt();
    let y_vals = integer_grid(mid.1 - max_t, mid.1 + max_t);
    // let mut y_vals_peek = y_vals.peekable();

    // +1 to account for inclusive bound
    // Saftey: this will never panic because the assertion in integer_grid bounds y_vals.start() at
    // y_vals.end() - 1, so min(output_height) = 0
    let output_height = usize::try_from(y_vals.end() - y_vals.start() + 1).unwrap();
    let max_width = (2.0 * semi_minor / ctx.sin_theta).ceil().abs() as usize + 1;

    let x_base = (pa.0.min(pb.0) - semi_minor - OFFSET_BUFFER).floor() as i64;
    let y_base = *y_vals.start();

    let mut values = Array2::<FresnelZonePoint>::default((output_height, max_width));
    let mut widths = Array1::<usize>::zeros(output_height);
    let mut offsets = Array1::<usize>::zeros(output_height);

    // Precompute the constant part of the earth-curvature correction (evaluated at the midpoint).
    let center_correction =
        (EARTH_RADIUS_USFT * EARTH_RADIUS_USFT - (major_axis / 2.0).powi(2)).sqrt();

    // Convert usft → inches, clamp to uint16
    let to_u16 = |v: f64| -> u16 { (v * 12.0).clamp(0.0, 65535.0) as u16 };

    for (i, y) in y_vals.enumerate() {
        let yf = y as f64;

        // E: 4×3 slice-plane matrix
        // col0=[1,0,0,0], col1=[0,0,1,0], col2=[mid_x, y, mid_z, 1]
        let e = SMatrix::<f64, 4, 3>::from_row_slice(&[
            1.0, 0.0, mid.0, 0.0, 0.0, yf, 0.0, 1.0, mid.2, 0.0, 0.0, 1.0,
        ]);

        // Conic section of the ellipsoid at this y plane: C = E^T Q_nys E
        let c_conic: Matrix3<f64> = e.transpose() * q_nys * e;

        let (c_norm, u, v) = match normalize_ellipse(&c_conic) {
            Some(x) => x,
            None => continue,
        };

        let half_w = match ellipse_half_width(&c_norm) {
            Some(x) => x,
            None => continue,
        };

        let ell_x_nys = u + mid.0;
        let x_grid_nys = integer_grid(ell_x_nys - half_w, ell_x_nys + half_w);
        if x_grid_nys.is_empty() {
            continue;
        }
        // Saftey: this will never panic because the assertion in integer_grid bounds
        // x_grid_nys.start() at x_grid_nys.end() - 1, so min(width) = 0
        let width = usize::try_from(x_grid_nys.end() - x_grid_nys.start() + 1).unwrap();

        let x_row_base = *x_grid_nys.start();

        // Shift x into the ellipse's local frame for sampling
        let x_grid_ell = x_grid_nys.clone().map(|x| x as f64 - ell_x_nys);
        let row_f64_lower_upper_iter = sample_conic(&c_norm, x_grid_ell);

        let ell_z_nys = v + mid.2;
        let dy = yf - mid.1;

        widths[i] = width;
        offsets[i] = (x_row_base - x_base) as usize;

        x_grid_nys
            .zip(row_f64_lower_upper_iter)
            .enumerate()
            .for_each(|(j, (xn, (lz, uz)))| {
                let lower_z_nys = lz + ell_z_nys;
                let upper_z_nys = uz + ell_z_nys;

                // Earth-curvature correction
                let dx = xn as f64 - mid.0;
                let axial = r1_dx * dx + r1_dy * dy;
                let correction = (EARTH_RADIUS_USFT * EARTH_RADIUS_USFT - axial * axial)
                    .max(0.0)
                    .sqrt()
                    - center_correction;

                values[[i, j]] = FresnelZonePoint::new(
                    to_u16(lower_z_nys - correction),
                    to_u16(upper_z_nys - correction),
                );
            });
    }

    FresnelZone::new(
        values,
        widths,
        offsets,
        NYSCoords2::new(x_base as f64, y_base as f64),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::point_evaluation::PointEvaluationInput;
    use crate::types::coords::{GPSCoords3, NYSCoords3};
    use crate::types::obstructions::ObstructionTypesFilter;
    use crate::util::coord_conversion::CoordinateConverter;
    use approx::assert_relative_eq;

    fn gps_to_nys(lat: f64, lon: f64, alt_m: f64) -> NYSCoords3 {
        CoordinateConverter::new().to_nys_plane3(&GPSCoords3::new(lat, lon, alt_m))
    }

    fn make_input(pa: NYSCoords3, pb: NYSCoords3, freq: f64) -> PointEvaluationInput {
        PointEvaluationInput::new(pa, pb, freq, ObstructionTypesFilter::All)
    }

    const REL_TOL: f64 = 1e-6;

    #[test]
    fn test_angle_context() {
        let delta: (f64, f64, f64) = (-57160.96194245259, 24456.04380098125, 32480.250000000004);
        let (dx, dy, dz) = delta;

        let tan_theta: f64 = -dy / dx;
        let tan_phi: f64 = -dz / dx;
        let theta = tan_theta.atan();
        let phi = tan_phi.atan();
        let tan_rho = tan_phi * theta.cos();
        let rho = tan_rho.atan();
        let tan_omega = theta.cos() / (theta.sin() * phi.cos());
        let omega = tan_omega.atan();

        let ctx = AngleContext::from_delta(delta);

        assert_relative_eq!(ctx.sin_theta, theta.sin(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.cos_theta, theta.cos(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.sin_phi, phi.sin(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.cos_phi, phi.cos(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.sin_rho, rho.sin(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.cos_rho, rho.cos(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.sin_omega, omega.sin(), max_relative = REL_TOL);
        assert_relative_eq!(ctx.cos_omega, omega.cos(), max_relative = REL_TOL);
    }

    #[test]
    fn test_rotation_ellipsoid_to_nys() {
        let pa = (1039747.7086964573, 176152.26368097877, 328.08333333333337);
        let pb = (982586.7467540047, 200608.30748196002, 32808.333333333336);
        let delta = (pb.0 - pa.0, pb.1 - pa.1, pb.2 - pa.2);

        let ctx = AngleContext::from_delta(delta);
        let result = rotation_ellipsoid_to_nys(&ctx);

        let expected = [
            [0.30312669, -0.81488729, 0.49403736, 0.0],
            [0.93725462, 0.34864563, 0.0, 0.0],
            [-0.17224396, 0.4630388, 0.86944068, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        for row in 0..4 {
            for col in 0..4 {
                assert_relative_eq!(
                    result[(row, col)],
                    expected[row][col],
                    max_relative = REL_TOL,
                    epsilon = 1e-6,
                );
            }
        }
    }

    #[test]
    fn test_construct_fresnel_quadratic() {
        let (q, semi_major, semi_minor) =
            construct_fresnel_quadratic(70145.8501170563, 5_000_000_000.0, 0.8);

        let expected_diag = [4.52942599e-04, 8.12933101e-10, 4.52942599e-04, -1.0];
        for (i, &exp) in expected_diag.iter().enumerate() {
            assert_relative_eq!(q[(i, i)], exp, max_relative = REL_TOL);
        }
        // Off-diagonal elements must be zero.
        for row in 0..4 {
            for col in 0..4 {
                if row != col {
                    assert_eq!(q[(row, col)], 0.0);
                }
            }
        }

        assert_relative_eq!(semi_major, 35072.97423698255, max_relative = REL_TOL);
        assert_relative_eq!(semi_minor, 46.987075610800986, max_relative = REL_TOL);
    }

    #[test]
    fn test_integer_grid() {
        let collect = |lo: f64, hi: f64| -> Vec<i64> { integer_grid(lo, hi).collect() };

        assert_eq!(collect(-3.1, 1.98), vec![-3, -2, -1, 0, 1]);
        assert_eq!(collect(-3.0, 2.0), vec![-3, -2, -1, 0, 1, 2]);
        assert_eq!(collect(1.0, 1.98), vec![1]);
        assert_eq!(collect(1.0, 1.0), vec![1]);
        assert_eq!(collect(2.0, 7.0), vec![2, 3, 4, 5, 6, 7]);
    }

    #[test]
    #[should_panic]
    fn test_integer_grid_invalid() {
        let _ = integer_grid(10.0, 0.0).collect::<Vec<_>>();
    }

    #[test]
    fn test_normalize_ellipse() {
        let c = Matrix3::new(
            0.00015217054611868413,
            0.00017090600184125422,
            1.5442065871587545,
            0.00017090600184125422,
            0.0003558296484122786,
            -0.8774557722289558,
            1.5442065871587545,
            -0.8774557722289558,
            57294.55752986111,
        );

        let (c_norm, u, v) = normalize_ellipse(&c).expect("normalize_ellipse returned None");

        let expected_norm = Matrix3::new(
            0.00015217054611868413,
            0.00017090600184125422,
            0.0,
            0.00017090600184125422,
            0.0003558296484122786,
            0.0,
            0.0,
            0.0,
            -0.0369624448723276,
        );

        for row in 0..3 {
            for col in 0..3 {
                assert_relative_eq!(
                    c_norm[(row, col)],
                    expected_norm[(row, col)],
                    epsilon = 1e-9
                );
            }
        }

        assert_relative_eq!(u, -28047.112648969174, max_relative = REL_TOL);
        assert_relative_eq!(v, 15937.052135928374, max_relative = REL_TOL);
    }

    #[test]
    fn test_nys_to_ellipsoid_transform() {
        let pa = (
            1039747.7086964573f64,
            176152.26368097877,
            328.08333333333337,
        );
        let pb = (982586.7467540047f64, 200608.30748196002, 32808.333333333336);
        let delta = (pb.0 - pa.0, pb.1 - pa.1, pb.2 - pa.2);
        let mid = (
            (pa.0 + pb.0) / 2.0,
            (pa.1 + pb.1) / 2.0,
            (pa.2 + pb.2) / 2.0,
        );
        let dist = (delta.0 * delta.0 + delta.1 * delta.1 + delta.2 * delta.2).sqrt();

        let ctx = AngleContext::from_delta(delta);
        let a_nys_to_ell = nys_to_ellipsoid_transform(mid, &ctx);
        let a_ell_to_nys = a_nys_to_ell.try_inverse().unwrap();

        let close = |a: f64, b: f64| assert_relative_eq!(a, b, epsilon = 1e-6);

        // Midpoint maps to the ellipsoid origin.
        let result = a_nys_to_ell * Vector4::new(mid.0, mid.1, mid.2, 1.0);
        close(result[0], 0.0);
        close(result[1], 0.0);
        close(result[2], 0.0);
        assert_relative_eq!(result[3], 1.0, epsilon = 1e-9);

        // Ellipsoid origin maps back to the midpoint.
        let result = a_ell_to_nys * Vector4::new(0.0, 0.0, 0.0, 1.0);
        close(result[0], mid.0);
        close(result[1], mid.1);
        close(result[2], mid.2);

        // Ellipsoid (0, +dist/2, 0) maps back to point_b.
        let result = a_ell_to_nys * Vector4::new(0.0, dist / 2.0, 0.0, 1.0);
        close(result[0], pb.0);
        close(result[1], pb.1);
        close(result[2], pb.2);

        // Ellipsoid (0, -dist/2, 0) maps back to point_a.
        let result = a_ell_to_nys * Vector4::new(0.0, -dist / 2.0, 0.0, 1.0);
        close(result[0], pa.0);
        close(result[1], pa.1);
        close(result[2], pa.2);
    }

    // --- compute_fresnel_zone smoke tests ---

    #[test]
    fn test_stress_test_fresnel_zone() {
        let input = make_input(
            gps_to_nys(40.650, -73.800, 100.0),
            gps_to_nys(40.7173, -74.0060, 10000.0),
            5_000_000_000.0,
        );
        let _zone = compute_fresnel_zone(&input, 0.8);
    }

    #[test]
    fn test_old_stress_test_fresnel_zone() {
        let input = make_input(
            gps_to_nys(40.650, -73.979, 100.0),
            gps_to_nys(40.7173, -74.0060, 100.0),
            2_400_000_000.0,
        );
        let _zone = compute_fresnel_zone(&input, 1.0);
    }

    #[test]
    fn test_east_west_long_fresnel_zone() {
        let input = make_input(
            gps_to_nys(40.650, -73.800, 100.0),
            gps_to_nys(40.650, -74.000, 100.0),
            5_000_000_000.0,
        );
        let _zone = compute_fresnel_zone(&input, 1.0);
    }

    #[test]
    fn test_north_south_long_fresnel_zone() {
        let input = make_input(
            gps_to_nys(40.650, -73.800, 100.0),
            gps_to_nys(40.8, -73.800, 100.0),
            5_000_000_000.0,
        );
        let _zone = compute_fresnel_zone(&input, 1.0);
    }

    #[test]
    fn test_new_fresnel_zone() {
        let input = make_input(
            gps_to_nys(40.81399261450678, -73.9576824966002, 100.0),
            gps_to_nys(40.81669146433694, -73.93829606722406, 100.0),
            5_000_000_000.0,
        );
        let _zone = compute_fresnel_zone(&input, 1.0);
    }

    #[test]
    fn test_fresnel_zone_empty_x_grid() {
        // At 24 GHz the semi-minor axis is tiny (~0.17 usft), so the x bounds can
        // span < 1 usft — no integer x falls inside and the row must be skipped, not panic.
        let input = make_input(
            gps_to_nys(40.861448, -73.907696, 76.0),
            gps_to_nys(40.830477, -73.941012, 80.0),
            24_000_000_000.0,
        );
        let _zone: FresnelZone = compute_fresnel_zone(&input, 1.0);
    }

    #[test]
    fn test_fresnel_zone_identical_easting_does_not_panic() {
        // When pa and pb share the same easting, delta.0 == 0.0 which would cause a divide-by-zero
        // inside AngleContext. The function shifts pb.0 by 0.1 usft to avoid this.
        let pa = NYSCoords3::new(1_000_000.0, 176_000.0, 300.0);
        let pb = NYSCoords3::new(1_000_000.0, 200_000.0, 500.0); // identical easting
        let input = make_input(pa, pb, 5_000_000_000.0);
        let _zone: FresnelZone = compute_fresnel_zone(&input, 1.0);
    }
}
