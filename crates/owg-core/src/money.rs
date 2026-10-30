use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Neg, Sub};

/// Fixed-point monetary amount in the scenario currency's minor unit.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Eq,
    Hash,
    JsonSchema,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
#[serde(transparent)]
pub struct Money(pub i64);

impl Money {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn minor_units(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, rhs: Self) -> OwgResult<Self> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(OwgError::MoneyOverflow)
    }

    pub fn checked_sub(self, rhs: Self) -> OwgResult<Self> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(OwgError::MoneyOverflow)
    }

    pub fn checked_mul(self, rhs: i64) -> OwgResult<Self> {
        self.0
            .checked_mul(rhs)
            .map(Self)
            .ok_or(OwgError::MoneyOverflow)
    }

    /// Exact rational multiplication with half-away-from-zero minor-unit
    /// rounding. The wider intermediate avoids overflow before division.
    pub fn mul_ratio(self, numerator: u32, denominator: u32) -> OwgResult<Self> {
        if denominator == 0 {
            return Err(OwgError::InvalidState(
                "zero monetary denominator".to_owned(),
            ));
        }
        let product = i128::from(self.0) * i128::from(numerator);
        let denominator = i128::from(denominator);
        let rounded = (product.abs() + denominator / 2) / denominator;
        let signed = if product < 0 { -rounded } else { rounded };
        i64::try_from(signed)
            .map(Self)
            .map_err(|_| OwgError::MoneyOverflow)
    }

    /// Multiply by a floating rate with deterministic half-away-from-zero rounding.
    pub fn mul_rate(self, rate: f64) -> OwgResult<Self> {
        if !rate.is_finite() {
            return Err(OwgError::InvalidState(
                "non-finite monetary rate".to_owned(),
            ));
        }
        let value = self.0 as f64 * rate;
        if value > i64::MAX as f64 || value < i64::MIN as f64 {
            return Err(OwgError::MoneyOverflow);
        }
        Ok(Self(value.round() as i64))
    }

    #[must_use]
    pub fn as_major_f64(self) -> f64 {
        self.0 as f64 / 100.0
    }

    pub fn from_major_f64(value: f64) -> OwgResult<Self> {
        if !value.is_finite() {
            return Err(OwgError::InvalidState(
                "non-finite monetary value".to_owned(),
            ));
        }
        let minor_units = value * 100.0;
        if minor_units > i64::MAX as f64 || minor_units < i64::MIN as f64 {
            return Err(OwgError::MoneyOverflow);
        }
        Ok(Self(minor_units.round() as i64))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let absolute = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02}", absolute / 100, absolute % 100)
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_add(rhs.0)
                .expect("Money addition overflow; use checked_add at trust boundaries"),
        )
    }
}

impl Sub for Money {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(
            self.0
                .checked_sub(rhs.0)
                .expect("Money subtraction overflow; use checked_sub at trust boundaries"),
        )
    }
}

impl Neg for Money {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(
            self.0
                .checked_neg()
                .expect("Money negation overflow at i64::MIN"),
        )
    }
}

impl From<i64> for Money {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Money;

    #[test]
    fn formats_minor_units() {
        assert_eq!(Money(12_345).to_string(), "123.45");
        assert_eq!(Money(-5).to_string(), "-0.05");
    }

    #[test]
    fn rounds_rates_deterministically() {
        assert_eq!(Money(101).mul_rate(0.5).unwrap(), Money(51));
        assert_eq!(Money(-101).mul_rate(0.5).unwrap(), Money(-51));
    }

    #[test]
    fn rational_rates_use_wide_intermediates_and_checked_rounding() {
        assert_eq!(
            Money(i64::MAX).mul_ratio(10_000, 10_000).unwrap(),
            Money(i64::MAX)
        );
        assert_eq!(Money(-101).mul_ratio(1, 2).unwrap(), Money(-51));
        assert!(Money(1).mul_ratio(1, 0).is_err());
        assert!(Money(i64::MAX).mul_ratio(2, 1).is_err());
    }
}
