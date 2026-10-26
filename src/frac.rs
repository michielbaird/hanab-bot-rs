use std::cmp::Ordering;
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs()
}

/// An exact fraction, always stored in lowest terms with a positive denominator.
///
/// (The Scala `Frac` reduced lazily after each operation and let the sign sit on either side;
/// normalising up front makes derived equality and hashing correct.)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frac {
    numer: i64,
    denom: i64,
}

impl Frac {
    pub const ZERO: Self = Self { numer: 0, denom: 1 };
    pub const ONE: Self = Self { numer: 1, denom: 1 };

    /// # Panics
    /// If `denom` is zero.
    pub fn new(numer: i64, denom: i64) -> Self {
        assert!(denom != 0, "denominator cannot be zero");
        if numer == 0 {
            return Self::ZERO;
        }
        let div = gcd(numer, denom) * denom.signum();
        Self {
            numer: numer / div,
            denom: denom / div,
        }
    }

    pub const fn numer(self) -> i64 {
        self.numer
    }

    pub const fn denom(self) -> i64 {
        self.denom
    }

    pub fn to_decimal(self) -> f64 {
        self.numer as f64 / self.denom as f64
    }
}

impl From<i64> for Frac {
    fn from(n: i64) -> Self {
        Self { numer: n, denom: 1 }
    }
}

impl From<i32> for Frac {
    fn from(n: i32) -> Self {
        Self::from(i64::from(n))
    }
}

impl Add for Frac {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(
            self.numer * rhs.denom + rhs.numer * self.denom,
            self.denom * rhs.denom,
        )
    }
}

impl Sub for Frac {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self + -rhs
    }
}

impl Mul for Frac {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new(self.numer * rhs.numer, self.denom * rhs.denom)
    }
}

impl Div for Frac {
    type Output = Self;
    /// # Panics
    /// If `rhs` is zero.
    fn div(self, rhs: Self) -> Self {
        Self::new(self.numer * rhs.denom, self.denom * rhs.numer)
    }
}

impl Neg for Frac {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            numer: -self.numer,
            denom: self.denom,
        }
    }
}

impl Add<i64> for Frac {
    type Output = Self;
    fn add(self, rhs: i64) -> Self {
        self + Self::from(rhs)
    }
}

impl Sub<i64> for Frac {
    type Output = Self;
    fn sub(self, rhs: i64) -> Self {
        self - Self::from(rhs)
    }
}

impl Mul<i64> for Frac {
    type Output = Self;
    fn mul(self, rhs: i64) -> Self {
        self * Self::from(rhs)
    }
}

impl Div<i64> for Frac {
    type Output = Self;
    fn div(self, rhs: i64) -> Self {
        self / Self::from(rhs)
    }
}

impl AddAssign for Frac {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Frac {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Sum for Frac {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl PartialOrd for Frac {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Frac {
    fn cmp(&self, other: &Self) -> Ordering {
        // Denominators are positive, so cross-multiplying preserves order.
        (i128::from(self.numer) * i128::from(other.denom))
            .cmp(&(i128::from(other.numer) * i128::from(self.denom)))
    }
}

impl fmt::Display for Frac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denom == 1 {
            write!(f, "{}", self.numer)
        } else {
            write!(f, "{}/{}", self.numer, self.denom)
        }
    }
}

impl fmt::Debug for Frac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: i64, d: i64) -> Frac {
        Frac::new(n, d)
    }

    #[test]
    fn reduces_on_construction() {
        assert_eq!(f(2, 4), f(1, 2));
        assert_eq!(f(6, 3), Frac::from(2));
        assert_eq!(f(0, 7), Frac::ZERO);
        assert_eq!(f(3, -6), f(-1, 2));
        assert_eq!(f(-3, -6), f(1, 2));
    }

    #[test]
    #[should_panic(expected = "denominator")]
    fn rejects_zero_denominator() {
        let _ = f(1, 0);
    }

    #[test]
    fn arithmetic() {
        assert_eq!(f(1, 2) + f(1, 3), f(5, 6));
        assert_eq!(f(1, 2) - f(1, 3), f(1, 6));
        assert_eq!(f(2, 3) * f(3, 4), f(1, 2));
        assert_eq!(f(1, 2) / f(1, 4), Frac::from(2));
        assert_eq!(f(1, 2) + 1, f(3, 2));
        assert_eq!(f(1, 2) * 4, Frac::from(2));
        assert_eq!(-f(1, 2), f(-1, 2));
        assert_eq!(f(1, 2) - f(1, 2), Frac::ZERO);
    }

    #[test]
    fn ordering() {
        assert!(f(1, 3) < f(1, 2));
        assert!(f(-1, 2) < f(1, 3));
        assert!(f(2, 4) >= f(1, 2));
        assert!(f(7, 2) > Frac::from(3));
        assert_eq!(f(1, 2).max(f(2, 3)), f(2, 3));
    }

    #[test]
    fn sums() {
        let total: Frac = [f(1, 2), f(1, 3), f(1, 6)].into_iter().sum();
        assert_eq!(total, Frac::ONE);
    }

    #[test]
    fn displays() {
        assert_eq!(f(3, 1).to_string(), "3");
        assert_eq!(f(3, 6).to_string(), "1/2");
        assert_eq!(f(-3, 6).to_string(), "-1/2");
        assert_eq!(f(1, 4).to_decimal(), 0.25);
    }
}
