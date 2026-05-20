use std::{usize};
use std::ops::RangeInclusive;
use derive_more::From;
use rocket::serde::{Deserialize, Serialize};
use crate::analysis::angle_context::AngleContext;
use crate::analysis::point_evaluation::PointEvaluationInput;
use nalgebra::{Matrix3, Matrix4, SMatrix, Vector4};
use ndarray::{Array1, Array2};
use crate::types::coords::NYSCoords2;
use crate::types::stairstep::StairStepGrid;

const OFFSET_BUFFER: f64 = 500.0;
const SPEED_OF_LIGHT_M_S: f64 = 299_792_458.0;
const USFT_PER_METER: f64 = 1.0 / 0.3048006096;
const EARTH_RADIUS_METERS: f64 = 6_369_160.0;
const EARTH_RADIUS_USFT: f64 = EARTH_RADIUS_METERS * USFT_PER_METER;

#[derive(Serialize, Deserialize, From, Default)]
pub struct FresnelZonePoint(u16, u16);

impl FresnelZonePoint {
    pub fn new(bottom: u16, top: u16) -> FresnelZonePoint { FresnelZonePoint(bottom, top) }
    pub fn bottom(&self) -> u16 { self.0 }
    pub fn top(&self) -> u16 { self.1 }
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
        1.0, 0.0, 0.0, offset.0, 0.0, 1.0, 0.0, offset.1, 0.0, 0.0, 1.0, offset.2, 0.0, 0.0,
        0.0, 1.0,
    )
}

/// Integer grid [ceil(lo), floor(hi)] inclusive.
fn integer_grid(lo: f64, hi: f64) -> RangeInclusive<i64> {
    let start = lo.ceil() as i64;
    let end = hi.floor() as i64;
    assert!(start <= end); // TODO: Validate this before call?
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
fn sample_conic<I: Iterator<Item = f64>>(c: &Matrix3<f64>, x_vals: I) -> impl Iterator<Item = (f64, f64)> {
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

pub fn compute_fresnel_zone(point_evaluation_input: &PointEvaluationInput, alpha: f64) -> FresnelZone {
    let pa: (f64, f64, f64) = point_evaluation_input.point_a().into();
    let pb: (f64, f64, f64) = point_evaluation_input.point_b().into();
    let delta = (pb.0 - pa.0, pb.1 - pa.1, pb.2 - pa.2);

    // TODO: Validate this assertion before calling this fn
    assert_ne!(delta.0, 0.0);

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

    // A_nys_to_ellipsoid = inv(T @ R)
    let a_ell_to_nys = translation_matrix(mid) * rotation_ellipsoid_to_nys(&ctx);
    let a_nys_to_ell = a_ell_to_nys
        .try_inverse()
        .unwrap(); // TODO: Should this be failable instead? Is this case even possible?

    // Q expressed in NYS: Q_nys = A^T Q_ell A
    let q_nys = a_nys_to_ell.transpose() * q_ellipsoid * a_nys_to_ell;

    // Y grid
    let max_t =
        ((semi_minor * ctx.sin_omega).powi(2) + (semi_major * ctx.cos_omega).powi(2)).sqrt();
    let y_vals = integer_grid(mid.1 - max_t, mid.1 + max_t);
    // let mut y_vals_peek = y_vals.peekable();

    // +1 to account for inclusive bound
    let output_height = usize::try_from(y_vals.end() - y_vals.start() + 1).unwrap(); // TODO: unwrap safety?
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

        let width = usize::try_from(x_grid_nys.end() - x_grid_nys.start() + 1).unwrap(); // TODO: unwrap safety?

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
                let correction =
                    (EARTH_RADIUS_USFT * EARTH_RADIUS_USFT - axial * axial).max(0.0).sqrt()
                        - center_correction;

                // TODO: Is i.j right? Should it be j,i?
                values[[i,j]] = FresnelZonePoint::new(
                    to_u16(lower_z_nys - correction),
                    to_u16(upper_z_nys - correction)
                );
            });
    }

    FresnelZone::new(values, widths, offsets, NYSCoords2::new(x_base as f64, y_base as f64))
}