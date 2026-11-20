//! Exact time and rational arithmetic.
//!
//! Subtitle timing is stored as an exact [`Rational`] number of seconds. No
//! floating point value is ever used to decide ordering or equality, which
//! makes framerate conversions (23.976 <-> 25, etc.) exact and shift/unshift
//! round trips lossless. Floating point is only used by the read-only
//! [`Rational::to_f64`] convenience accessor.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Sub};

use crate::error::{invalid_time, Error, Result};

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

/// An exact rational number, always kept in reduced form with a positive
/// denominator.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Rational {
    num: i64,
    den: u64,
}

impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };

    /// Creates a reduced rational, rejecting a zero denominator.
    pub fn new(num: i64, den: u64) -> Result<Self> {
        if den == 0 {
            return Err(Error::ZeroDenominator);
        }
        let divisor = gcd(num.unsigned_abs(), den);
        Ok(Self { num: num / divisor as i64, den: den / divisor })
    }

    /// Creates an integer rational.
    pub const fn from_int(value: i64) -> Self {
        Self { num: value, den: 1 }
    }

    pub const fn num(self) -> i64 {
        self.num
    }

    pub const fn den(self) -> u64 {
        self.den
    }

    fn from_i128(num: i128, den: u128) -> Result<Self> {
        let num = i64::try_from(num).map_err(|_| Error::ArithmeticOverflow)?;
        let den = u64::try_from(den).map_err(|_| Error::ArithmeticOverflow)?;
        Self::new(num, den)
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        Self::from_i128(
            self.num as i128 * rhs.den as i128 + rhs.num as i128 * self.den as i128,
            self.den as u128 * rhs.den as u128,
        )
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        Self::from_i128(
            self.num as i128 * rhs.den as i128 - rhs.num as i128 * self.den as i128,
            self.den as u128 * rhs.den as u128,
        )
    }

    pub fn checked_mul(self, rhs: Self) -> Result<Self> {
        Self::from_i128(self.num as i128 * rhs.num as i128, self.den as u128 * rhs.den as u128)
    }

    pub fn checked_div(self, rhs: Self) -> Result<Self> {
        if rhs.num == 0 {
            return Err(Error::ZeroDenominator);
        }
        // (a/b) / (c/d) = (a*d) / (b*c). The denominator is forced positive,
        // so only the divisor's sign has to move onto the numerator: the
        // dividend's sign is already carried by `self.num * rhs.den`. Flipping
        // on the XOR of the two signs (the old bug) inverted the quotient
        // whenever the dividend was negative.
        let divisor_sign = if rhs.num < 0 { -1_i128 } else { 1_i128 };
        Self::from_i128(
            (self.num as i128 * rhs.den as i128) * divisor_sign,
            self.den as u128 * rhs.num.unsigned_abs() as u128,
        )
    }

    /// Rounds to the nearest integer, with halves rounded away from zero.
    pub fn round(self) -> i64 {
        let negative = self.num < 0;
        let magnitude = self.num.unsigned_abs() as u128;
        let rounded = (magnitude + self.den as u128 / 2) / self.den as u128;
        let rounded = i64::try_from(rounded).unwrap_or(i64::MAX);
        if negative {
            -rounded
        } else {
            rounded
        }
    }

    /// Approximation for display only; never used for comparisons.
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.num as i128 * other.den as i128).cmp(&(other.num as i128 * self.den as i128))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

/// The most negative/positive exactly representable rational. The operator
/// impls saturate to these instead of panicking: arithmetic inside a library
/// must never abort the host process. Callers that need to detect overflow
/// keep using the fallible `checked_*` methods.
fn saturating(negative: bool) -> Rational {
    if negative {
        Rational::from_int(i64::MIN + 1)
    } else {
        Rational::from_int(i64::MAX)
    }
}

impl Add for Rational {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        match self.checked_add(rhs) {
            Ok(value) => value,
            Err(_) => {
                let left = (self.num as i128).saturating_mul(rhs.den as i128);
                let right = (rhs.num as i128).saturating_mul(self.den as i128);
                saturating(left.saturating_add(right) < 0)
            }
        }
    }
}

impl Sub for Rational {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        match self.checked_sub(rhs) {
            Ok(value) => value,
            Err(_) => {
                let left = (self.num as i128).saturating_mul(rhs.den as i128);
                let right = (rhs.num as i128).saturating_mul(self.den as i128);
                saturating(left.saturating_sub(right) < 0)
            }
        }
    }
}

impl Mul for Rational {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        match self.checked_mul(rhs) {
            Ok(value) => value,
            Err(_) => saturating((self.num < 0) != (rhs.num < 0)),
        }
    }
}

impl Div for Rational {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        match self.checked_div(rhs) {
            Ok(value) => value,
            Err(Error::ZeroDenominator) if self.num == 0 => Self::ZERO,
            Err(_) => saturating((self.num < 0) != (rhs.num < 0)),
        }
    }
}

/// An exact point in time, stored as a rational number of seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time(Rational);

impl Time {
    pub const ZERO: Self = Self(Rational::ZERO);

    pub fn from_millis(millis: i64) -> Self {
        Self(Rational::new(millis, 1_000).expect("constant denominator"))
    }

    pub fn from_centis(centis: i64) -> Self {
        Self(Rational::new(centis, 100).expect("constant denominator"))
    }

    pub fn from_rational_seconds(seconds: Rational) -> Self {
        Self(seconds)
    }

    pub fn from_hms_millis(hours: u64, minutes: u32, seconds: u32, millis: u32) -> Result<Self> {
        if minutes > 59 || seconds > 59 || millis > 999 {
            return Err(invalid_time(
                "hms",
                &format!("{hours}:{minutes}:{seconds}.{millis}"),
                "minute, second or millisecond out of range",
            ));
        }
        let total = hours as i128 * 3_600_000
            + minutes as i128 * 60_000
            + seconds as i128 * 1_000
            + millis as i128;
        let total = i64::try_from(total).map_err(|_| Error::ArithmeticOverflow)?;
        Ok(Self::from_millis(total))
    }

    pub fn as_rational(self) -> Rational {
        self.0
    }

    /// Approximation in seconds for display only.
    pub fn as_secs_f64(self) -> f64 {
        self.0.to_f64()
    }

    /// Exact milliseconds as a rational (sub-millisecond times are preserved).
    pub fn as_millis(self) -> Rational {
        self.0 * Rational::from_int(1_000)
    }

    /// Formats as `HH:MM:SS,mmm`, the strict SRT representation.
    pub fn format_srt(self) -> String {
        self.format_components(',', 3)
    }

    /// Formats as `HH:MM:SS.mmm`, the strict WebVTT representation.
    pub fn format_vtt(self) -> String {
        self.format_components('.', 3)
    }

    /// Formats as `H:MM:SS.cc`, the strict ASS/SSA representation.
    pub fn format_ass(self) -> String {
        self.format_components('.', 2)
    }

    fn format_components(self, separator: char, fraction_digits: u32) -> String {
        let scale = if fraction_digits == 3 { 1_000 } else { 100 };
        let total = (self.0 * Rational::from_int(scale)).round();
        // Subtitle file formats have no representation for times before zero.
        // Writers must never emit "-00:00:05,000", which our own parser
        // rejects; clamp defensively so every writer stays round-trippable.
        let total = total.max(0);
        let units = total as u128;
        let fraction_base = 10_u128.pow(fraction_digits);
        let fraction = units % fraction_base;
        let seconds_total = units / fraction_base;
        let seconds = seconds_total % 60;
        let minutes = (seconds_total / 60) % 60;
        let hours = seconds_total / 3_600;
        format!(
            "{:02}:{:02}:{:02}{}{:0width$}",
            hours,
            minutes,
            seconds,
            separator,
            fraction,
            width = fraction_digits as usize
        )
    }

    /// Parses an SRT timestamp; both `,` and `.` are accepted as the
    /// millisecond separator and 1-3 fractional digits are tolerated.
    pub fn parse_srt(input: &str) -> Result<Self> {
        parse_time(input, "SRT", 3)
    }

    /// Parses a WebVTT timestamp, including the `MM:SS.mmm` short form.
    pub fn parse_vtt(input: &str) -> Result<Self> {
        parse_time(input, "WebVTT", 3)
    }

    /// Parses an ASS/SSA timestamp such as `0:00:01.23`.
    pub fn parse_ass(input: &str) -> Result<Self> {
        parse_time(input, "ASS", 2)
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_srt())
    }
}

fn parse_time(input: &str, format: &'static str, fraction_digits: u32) -> Result<Time> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(invalid_time(format, input, "empty timestamp"));
    }

    let parts: Vec<&str> = trimmed.split(':').map(str::trim).collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(invalid_time(format, input, "expected MM:SS or HH:MM:SS"));
    }

    let (seconds_text, fraction_value) = {
        let last = parts[parts.len() - 1];
        let split_at = last.find(['.', ',']);
        match split_at {
            None => (last, 0),
            Some(index) => {
                let fraction = &last[index + 1..];
                if !fraction.is_empty() && !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid_time(format, input, "fraction is not numeric"));
                }
                let mut digits: String = fraction.chars().take(fraction_digits as usize).collect();
                while digits.len() < fraction_digits as usize {
                    digits.push('0');
                }
                let value = if digits.is_empty() {
                    0
                } else {
                    digits
                        .parse::<u64>()
                        .map_err(|_| invalid_time(format, input, "fraction is not numeric"))?
                };
                (&last[..index], value)
            }
        }
    };

    if seconds_text.is_empty() || !seconds_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_time(format, input, "seconds are not numeric"));
    }
    let seconds: u64 = seconds_text
        .parse()
        .map_err(|_| invalid_time(format, input, "seconds are out of range"))?;

    let (hours, minutes) = if parts.len() == 3 {
        let hours_text = parts[0];
        let minutes_text = parts[1];
        if hours_text.is_empty()
            || !hours_text.bytes().all(|byte| byte.is_ascii_digit())
            || minutes_text.is_empty()
            || !minutes_text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid_time(format, input, "hours and minutes must be numeric"));
        }
        let hours: u64 = hours_text
            .parse()
            .map_err(|_| invalid_time(format, input, "hours are out of range"))?;
        let minutes: u64 = minutes_text
            .parse()
            .map_err(|_| invalid_time(format, input, "minutes are out of range"))?;
        if minutes > 59 {
            return Err(invalid_time(format, input, "minutes must be below 60"));
        }
        (hours, minutes)
    } else {
        let minutes_text = parts[0];
        if minutes_text.is_empty() || !minutes_text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_time(format, input, "minutes must be numeric"));
        }
        let minutes: u64 = minutes_text
            .parse()
            .map_err(|_| invalid_time(format, input, "minutes are out of range"))?;
        (0, minutes)
    };

    if seconds > 59 {
        return Err(invalid_time(format, input, "seconds must be below 60"));
    }

    let scale = 10_u64.pow(fraction_digits);
    let total = (hours as u128 * 3_600 + minutes as u128 * 60 + seconds as u128) * scale as u128
        + fraction_value as u128;
    let total = i64::try_from(total).map_err(|_| Error::ArithmeticOverflow)?;

    Ok(if fraction_digits == 3 { Time::from_millis(total) } else { Time::from_centis(total) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::prop_assert_eq;

    #[test]
    fn rationals_are_reduced() {
        assert_eq!(Rational::new(4, 8).unwrap().to_string(), "1/2");
        assert_eq!(Rational::new(-6, 9).unwrap().to_string(), "-2/3");
        assert_eq!(Rational::new(0, 99).unwrap().to_string(), "0");
        assert_eq!(Rational::new(7, 1).unwrap().to_string(), "7");
    }

    #[test]
    fn rational_zero_denominator_is_rejected() {
        assert!(matches!(Rational::new(1, 0), Err(Error::ZeroDenominator)));
    }

    #[test]
    fn exact_arithmetic() {
        let third = Rational::new(1, 3).unwrap();
        let sixth = Rational::new(1, 6).unwrap();
        assert_eq!((third + sixth).to_string(), "1/2");
        assert_eq!((third - sixth).to_string(), "1/6");
        assert_eq!((third * Rational::new(3, 1).unwrap()).to_string(), "1");
        assert_eq!((sixth / Rational::new(2, 3).unwrap()).to_string(), "1/4");
        assert!(matches!(third.checked_div(Rational::ZERO), Err(Error::ZeroDenominator)));
    }

    #[test]
    fn division_sign_is_correct_for_every_sign_combination() {
        // Hand-derived sign table: a/b has sign(a) * sign(b). The old
        // implementation built the quotient from |divisor| and then flipped
        // the sign whenever dividend and divisor differed, which produced
        // the OPPOSITE sign whenever the dividend was negative:
        //   (-1)/(-2) -> -1/2 instead of  1/2
        //   (-1)/2    ->  1/2 instead of -1/2
        // This flipped every cue of a two-point sync whose target times ran
        // in descending order.
        let one = Rational::from_int(1);
        let minus_one = Rational::from_int(-1);
        let two = Rational::from_int(2);
        let minus_two = Rational::from_int(-2);
        assert_eq!(minus_one.checked_div(minus_two).unwrap().to_string(), "1/2");
        assert_eq!(minus_one.checked_div(two).unwrap().to_string(), "-1/2");
        assert_eq!(one.checked_div(minus_two).unwrap().to_string(), "-1/2");
        assert_eq!(one.checked_div(two).unwrap().to_string(), "1/2");
        assert_eq!(
            Rational::new(-3, 4).unwrap().checked_div(Rational::new(3, 4).unwrap()).unwrap(),
            Rational::from_int(-1)
        );
        assert_eq!(
            Rational::new(-3, 4).unwrap().checked_div(Rational::new(-3, 4).unwrap()).unwrap(),
            Rational::ONE
        );
    }

    #[test]
    fn operator_impls_saturate_instead_of_panicking() {
        // Library code must never panic on arithmetic (review opinion item):
        // the operator impls saturate at the extreme representable values.
        let huge = Rational::new(i64::MAX, 1).unwrap();
        let minus_huge = Rational::new(i64::MIN + 1, 1).unwrap();
        assert_eq!(huge + huge, huge);
        assert_eq!(minus_huge + minus_huge, minus_huge);
        assert_eq!(minus_huge - huge, minus_huge);
        assert_eq!(huge * huge, huge);
        assert_eq!(minus_huge * huge, minus_huge);
        // Division by zero saturates to the extreme value with the
        // dividend's sign; a zero dividend stays zero.
        assert_eq!(huge / Rational::ZERO, huge);
        assert_eq!(minus_huge / Rational::ZERO, minus_huge);
        assert_eq!(Rational::ZERO / Rational::ZERO, Rational::ZERO);
    }

    #[test]
    fn fractions_order_exactly() {
        let one_third = Rational::new(1, 3).unwrap();
        let three_tenths = Rational::new(3, 10).unwrap();
        assert!(one_third > three_tenths);
        assert!(three_tenths < one_third);
    }

    #[test]
    fn rounding_is_away_from_zero() {
        assert_eq!(Rational::new(5, 2).unwrap().round(), 3);
        assert_eq!(Rational::new(-5, 2).unwrap().round(), -3);
        assert_eq!(Rational::new(1, 3).unwrap().round(), 0);
    }

    #[test]
    fn hms_round_trips_through_srt() {
        let time = Time::from_hms_millis(1, 2, 3, 4).unwrap();
        assert_eq!(time.format_srt(), "01:02:03,004");
        assert_eq!(Time::parse_srt(&time.format_srt()).unwrap(), time);
        assert_eq!(Time::parse_vtt(&time.format_vtt()).unwrap(), time);
    }

    #[test]
    fn ass_uses_centiseconds() {
        let time = Time::from_hms_millis(0, 1, 2, 300).unwrap();
        assert_eq!(time.format_ass(), "00:01:02.30");
        assert_eq!(Time::parse_ass(&time.format_ass()).unwrap(), time);
        assert_eq!(Time::parse_ass("0:00:01.5").unwrap(), Time::from_millis(1_500));
    }

    #[test]
    fn partial_fractions_are_padded() {
        assert_eq!(Time::parse_srt("00:00:01,5").unwrap(), Time::from_millis(1_500));
        assert_eq!(Time::parse_vtt("00:12.34").unwrap(), Time::from_millis(12_340));
        assert_eq!(Time::parse_srt("00:00:01,1234").unwrap(), Time::from_millis(1_123));
    }

    #[test]
    fn invalid_timestamps_are_rejected() {
        assert!(Time::parse_srt("not a time").is_err());
        assert!(Time::parse_srt("00:60:00,000").is_err());
        assert!(Time::parse_srt("00:00:60,000").is_err());
        assert!(Time::parse_srt("").is_err());
        assert!(Time::parse_vtt("1:2:3:4.000").is_err());
        assert!(Time::parse_ass("0:00:aa.00").is_err());
        assert!(Time::from_hms_millis(0, 60, 0, 0).is_err());
    }

    #[test]
    fn framerate_scaling_is_exact() {
        // 23.976 fps == 24000/1001; scaling to 25 fps multiplies by 1001/960.
        let one_second = Time::from_millis(1_000).as_rational();
        let scaled = one_second * Rational::new(1_001, 960).unwrap();
        assert_eq!(scaled.to_string(), "1001/960");
        assert_eq!(Time::from_rational_seconds(scaled).as_rational().num(), 1_001);
    }

    proptest::proptest! {
        #[test]
        fn shift_then_unshift_is_identity(millis in -86_400_000i64..86_400_000, shift in -3_600_000i64..3_600_000) {
            let original = Time::from_millis(millis);
            let shifted = original.as_rational() + Rational::from_int(shift) / Rational::from_int(1000);
            let unshifted = shifted - Rational::from_int(shift) / Rational::from_int(1000);
            prop_assert_eq!(unshifted, original.as_rational());
        }

        /// Independent oracle: the same fraction arithmetic implemented from
        /// scratch on i128 inside the test (cross-multiplied, then reduced by
        /// gcd), never via `Rational` itself. All four operations must agree
        /// with it exactly, for every sign combination.
        #[test]
        fn rational_arithmetic_matches_independent_i128_math(
            an in -9_000i64..9_000, ad in 1u64..9_000,
            bn in -9_000i64..9_000, bd in 1u64..9_000,
        ) {
            fn reduced(mut num: i128, mut den: i128) -> (i128, i128) {
                assert!(den != 0);
                if den < 0 {
                    num = -num;
                    den = -den;
                }
                let mut a = num.unsigned_abs();
                let mut b = den as u128;
                while b != 0 {
                    let remainder = a % b;
                    a = b;
                    b = remainder;
                }
                let divisor = a as i128;
                if divisor != 0 {
                    num /= divisor;
                    den /= divisor;
                }
                (num, den)
            }

            let a = Rational::new(an, ad).unwrap();
            let b = Rational::new(bn, bd).unwrap();

            let (sum_num, sum_den) =
                reduced(an as i128 * bd as i128 + bn as i128 * ad as i128, ad as i128 * bd as i128);
            let sum = a.checked_add(b).unwrap();
            prop_assert_eq!((sum.num() as i128, sum.den() as i128), (sum_num, sum_den));

            let (difference_num, difference_den) =
                reduced(an as i128 * bd as i128 - bn as i128 * ad as i128, ad as i128 * bd as i128);
            let difference = a.checked_sub(b).unwrap();
            prop_assert_eq!(
                (difference.num() as i128, difference.den() as i128),
                (difference_num, difference_den)
            );

            let (product_num, product_den) = reduced(an as i128 * bn as i128, ad as i128 * bd as i128);
            let product = a.checked_mul(b).unwrap();
            prop_assert_eq!((product.num() as i128, product.den() as i128), (product_num, product_den));

            if bn != 0 {
                let (quotient_num, quotient_den) =
                    reduced(an as i128 * bd as i128, ad as i128 * bn as i128);
                let quotient = a.checked_div(b).unwrap();
                prop_assert_eq!(
                    (quotient.num() as i128, quotient.den() as i128),
                    (quotient_num, quotient_den)
                );
            }
        }
    }
}
