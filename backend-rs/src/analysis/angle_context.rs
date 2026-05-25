
#[inline]
fn sin_of_atan(x: f64) -> f64 {
    x / (1.0 + x * x).sqrt()
}

#[inline]
fn cos_of_atan(x: f64) -> f64 {
    1.0 / (1.0 + x * x).sqrt()
}

#[derive(Debug)]
pub struct AngleContext {
    pub sin_theta: f64,
    pub cos_theta: f64,
    pub sin_phi: f64,
    pub cos_phi: f64,
    pub sin_rho: f64,
    pub cos_rho: f64,
    pub sin_omega: f64,
    pub cos_omega: f64,
}

impl AngleContext {
    pub fn from_delta(delta: (f64, f64, f64)) -> Self {
        let tan_theta = -delta.1 / delta.0;
        let sin_theta = sin_of_atan(tan_theta);
        let cos_theta = cos_of_atan(tan_theta);

        let tan_phi = -delta.2 / delta.0;
        let sin_phi = sin_of_atan(tan_phi);
        let cos_phi = cos_of_atan(tan_phi);

        let tan_rho = tan_phi * cos_theta;
        let sin_rho = sin_of_atan(tan_rho);
        let cos_rho = cos_of_atan(tan_rho);

        let tan_omega = cos_theta / (sin_theta * cos_phi);
        let sin_omega = sin_of_atan(tan_omega);
        let cos_omega = cos_of_atan(tan_omega);

        AngleContext {
            sin_theta,
            cos_theta,
            sin_phi,
            cos_phi,
            sin_rho,
            cos_rho,
            sin_omega,
            cos_omega,
        }
    }
}
