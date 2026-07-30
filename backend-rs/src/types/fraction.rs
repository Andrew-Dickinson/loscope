use rocket::serde::{Deserialize, Serialize};
use wincode::{SchemaRead, SchemaWrite};

/// A fraction in `[0.0, 1.0]`, fixed-point encoded as a single byte.
///
/// Values are quantized so that `0.0` maps to `0` and every other value in
/// `(0.0, 1.0]` maps to `1..=255`. This keeps the zero/non-zero distinction
/// exact (unlike naive rounding, which would flush small positive fractions
/// to zero), while still packing 256 levels of precision into one byte
/// instead of the 8 bytes an `f64` needs.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
    SchemaWrite, SchemaRead,
)]
pub struct FractionU8(u8);

impl FractionU8 {
    pub const ZERO: FractionU8 = FractionU8(0);
    pub const ONE: FractionU8 = FractionU8(255);

    /// Quantizes `v` into a `FractionU8`. `v` is clamped to `[0.0, 1.0]`.
    pub fn new(v: f64) -> Self {
        if v <= 0.0 {
            Self::ZERO
        } else if v >= 1.0 {
            Self::ONE
        } else {
            FractionU8(1 + (v * 254.0).floor() as u8)
        }
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl From<FractionU8> for f64 {
    fn from(f: FractionU8) -> f64 {
        f.0 as f64 / 255.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_round_trips_exactly() {
        assert!(FractionU8::new(0.0).is_zero());
    }

    #[test]
    fn small_positive_values_stay_nonzero() {
        assert!(!FractionU8::new(0.0001).is_zero());
        assert!(!FractionU8::new(f64::MIN_POSITIVE).is_zero());
    }

    #[test]
    fn one_round_trips_exactly() {
        assert_eq!(FractionU8::new(1.0), FractionU8::ONE);
        assert_eq!(f64::from(FractionU8::new(1.0)), 1.0);
    }

    #[test]
    fn ordering_matches_float_ordering() {
        assert!(FractionU8::new(0.1) < FractionU8::new(0.5));
        assert!(FractionU8::new(0.5) < FractionU8::new(0.9));
        assert!(FractionU8::ZERO < FractionU8::new(0.001));
    }

    #[test]
    fn clamps_out_of_range_values() {
        assert_eq!(FractionU8::new(-1.0), FractionU8::ZERO);
        assert_eq!(FractionU8::new(2.0), FractionU8::ONE);
    }
}
