//! wkhtmltopdf-rs: fast paths for the two kinds of non-integer reals that make up most of a
//! content stream, each writing exactly what `ryu::Buffer::format` writes:
//!
//! - colour components, `k / 255.0` for a byte `k`: written from a table built with ryu;
//! - coordinates with a short decimal expansion (`72.75`, `0.5`, `183.6`): written from the
//!   nearest integer multiple of `10^-d`, for the smallest `d <= 4` whose value rounds back to
//!   the `f32` (ryu's shortest representation), tested exactly.
//!
//! Anything else (more decimals, ties, powers of two, tiny or huge values) is left to ryu.
//! `tests::matches_ryu` compares both paths with ryu; the short-decimal path was also checked
//! against ryu for every `f32` in `[1e-3, 2^23)` and its negation.

use std::sync::OnceLock;

/// Writes `value` if it is `k / 255.0` for a byte `k`.
#[inline]
pub(crate) fn write_byte_fraction(out: &mut Vec<u8>, value: f32) -> bool {
    static TABLE: OnceLock<Vec<([u8; 16], u8)>> = OnceLock::new();
    if !(value > 0.0 && value < 1.0) {
        return false;
    }
    let k = (value * 255.0).round();
    if k as u8 as f32 / 255.0 != value {
        return false;
    }
    let table = TABLE.get_or_init(|| {
        let mut buf = ryu::Buffer::new();
        (0..=255u8)
            .map(|k| {
                let s = buf.format(k as f32 / 255.0).as_bytes();
                let mut a = [0; 16];
                a[..s.len()].copy_from_slice(s);
                (a, s.len() as u8)
            })
            .collect()
    });
    let (s, n) = &table[k as usize];
    out.extend_from_slice(&s[..usize::from(*n)]);
    true
}

/// Writes `value` (not an integer) if its shortest round-trip representation has at most
/// four decimals.
#[inline]
pub(crate) fn write_short_decimal(out: &mut Vec<u8>, value: f32) -> bool {
    const POW10: [f64; 5] = [1.0, 10.0, 100.0, 1000.0, 10000.0];
    let bits = value.to_bits();
    let mantissa = bits & 0x7F_FFFF;
    let exp = (bits >> 23) & 0xFF;
    // A power of two has an asymmetric rounding interval; subnormals, infinities, NaN.
    if mantissa == 0 || exp == 0 || exp == 0xFF {
        return false;
    }
    let abs = f64::from(value.abs());
    if !(1e-3..8_388_608.0).contains(&abs) {
        return false;
    }
    // Half the distance to either neighbour, exact.
    let half_ulp = f64::from_bits((u64::from(exp) + 1023 - 127 - 23) << 52) / 2.0;
    let even = mantissa & 1 == 0;
    // The nearest multiple of 10^-d, and whether it parses back to `value` (bounds: round
    // half to even); None on a tie, left to ryu's tie-breaking.
    let nearest = |d: usize| {
        let p = POW10[d];
        // Exact: a 24-bit significand times a 14-bit integer.
        let y = abs * p;
        let m = y.round();
        // Exact too: `m` and `y` are less than one apart.
        let diff = (m - y).abs();
        let bound = half_ulp * p;
        (diff != 0.5).then_some((m as u64, diff < bound || (diff == bound && even)))
    };
    // A value with fewer than 4 decimals also has 4: one test rejects the long ones.
    if !matches!(nearest(4), Some((_, true))) {
        return false;
    }
    for d in 1..=4 {
        match nearest(d) {
            None => return false,
            Some((_, false)) => continue,
            Some((m, true)) => {
                let p = POW10[d] as u64;
                if value < 0.0 {
                    out.push(b'-');
                }
                out.extend_from_slice(itoa::Buffer::new().format(m / p).as_bytes());
                out.push(b'.');
                let mut digits = [b'0'; 4];
                let mut frac = m % p;
                for digit in digits[..d].iter_mut().rev() {
                    *digit = b'0' + (frac % 10) as u8;
                    frac /= 10;
                }
                out.extend_from_slice(&digits[..d]);
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(value: f32) {
        let mut out = Vec::new();
        if write_byte_fraction(&mut out, value) || write_short_decimal(&mut out, value) {
            assert_eq!(
                std::str::from_utf8(&out).unwrap(),
                ryu::Buffer::new().format(value),
                "{value:?}"
            );
        }
    }

    #[test]
    fn matches_ryu() {
        for k in 0..=255u8 {
            check(k as f32 / 255.0);
        }
        for v in [0.5, 0.25, 72.75, 183.6, 0.001, 0.0015, 1.1, 8388607.5, 0.1, 0.3, 474.00003] {
            check(v);
            check(-v);
        }
        // A spread of bit patterns over the whole fast range.
        let (lo, hi) = (1e-4f32.to_bits(), 9_000_000f32.to_bits());
        let mut bits = lo;
        while bits < hi {
            check(f32::from_bits(bits));
            check(-f32::from_bits(bits));
            bits += 997;
        }
        // Integers (`push_decimal` forces the point: `3.0`).
        for n in 1..100_000 {
            check(n as f32);
            check(-(n as f32));
        }
        // Every value with up to three decimals below 2048.
        for n in 1..2_048_000u32 {
            check(n as f32 / 1000.0);
        }
    }
}
