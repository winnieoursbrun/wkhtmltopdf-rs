//! Helpers reproducing the Qt 4.8 number conversions that wkhtmltopdf relies on
//! (`QString::toInt`, `QString::toDouble`, `QString::toFloat`, `QString::number`,
//! `QString::arg(double)`, `QTextStream << double`).
//!
//! Ports the behaviour used by upstream `src/shared/arghandler.inl` and
//! `src/lib/pdfsettings.cc`; Qt itself is not part of the upstream tree.

/// Whitespace as understood by `QChar::isSpace` for the characters that can
/// reach us from argv.
fn is_qt_space(c: char) -> bool {
    c.is_whitespace()
}

fn trim(s: &str) -> &str {
    s.trim_matches(is_qt_space)
}

/// `QString::toInt(&ok)` with base 10: surrounding whitespace allowed, an
/// optional sign, at least one digit, and the value must fit in an `i32`.
pub fn to_int(s: &str) -> Option<i32> {
    let t = trim(s);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let neg = t.starts_with('-');
    // Accumulate in i64 with saturation to detect overflow like strtoll would.
    let mut v: i64 = 0;
    for b in digits.bytes() {
        v = v.saturating_mul(10).saturating_add(i64::from(b - b'0'));
        if v > i64::from(i32::MAX) + 1 {
            return None;
        }
    }
    let v = if neg { -v } else { v };
    i32::try_from(v).ok()
}

/// `QString::toDouble(&ok)`: surrounding whitespace allowed, optional sign,
/// decimal digits with an optional fraction, optional exponent. Overflow to
/// infinity is an error.
pub fn to_double(s: &str) -> Option<f64> {
    let t = trim(s);
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - f;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let v: f64 = t.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// `QString::toFloat(&ok)`: `toDouble`, then rejected when outside the float
/// range.
pub fn to_float(s: &str) -> Option<f32> {
    let d = to_double(s)?;
    if d.abs() > f64::from(f32::MAX) {
        return None;
    }
    Some(d as f32)
}

/// `QString::number(double)` / `QString::arg(double)` / `QTextStream << double`
/// with the default format: `%g` with 6 significant digits.
pub fn number_g(v: f64) -> String {
    format_g(v, 6)
}

/// C `printf("%.*g")` formatting, with Qt's two-digit minimum exponent.
pub fn format_g(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let p = precision.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // Let Rust round to p significant digits, then read the exponent back.
    let sci = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = sci.split_once('e').expect("exponent present");
    let exp: i32 = exp.parse().expect("valid exponent");
    if exp < -4 || exp >= p as i32 {
        let m = strip_zeros(mantissa);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{m}e{sign}{:02}", exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{v:.decimals$}")).to_string()
    }
}

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int_accepts_sign_and_whitespace() {
        assert_eq!(to_int("42"), Some(42));
        assert_eq!(to_int(" -7 "), Some(-7));
        assert_eq!(to_int("+3"), Some(3));
        assert_eq!(to_int("2147483647"), Some(i32::MAX));
        assert_eq!(to_int("-2147483648"), Some(i32::MIN));
    }

    #[test]
    fn to_int_rejects_garbage() {
        for s in ["", " ", "1.5", "0x10", "12a", "2147483648", "--1", "+"] {
            assert_eq!(to_int(s), None, "{s:?}");
        }
    }

    #[test]
    fn to_double_forms() {
        assert_eq!(to_double("1.5"), Some(1.5));
        assert_eq!(to_double(".5"), Some(0.5));
        assert_eq!(to_double("5."), Some(5.0));
        assert_eq!(to_double("-1e3"), Some(-1000.0));
        assert_eq!(to_double(" 2 "), Some(2.0));
        for s in ["", ".", "e5", "1e", "1,5", "abc", "1e999"] {
            assert_eq!(to_double(s), None, "{s:?}");
        }
    }

    #[test]
    fn to_float_range() {
        assert_eq!(to_float("0.8"), Some(0.8));
        assert_eq!(to_float("1e39"), None);
    }

    #[test]
    fn number_g_matches_qt() {
        assert_eq!(number_g(f64::from(0.8f32)), "0.8");
        assert_eq!(number_g(f64::from(0.8f32) * 100.0), "80");
        assert_eq!(number_g(1.0), "1");
        assert_eq!(number_g(0.0), "0");
        assert_eq!(number_g(20.0), "20");
        assert_eq!(number_g(0.75), "0.75");
        assert_eq!(number_g(1e6), "1e+06");
        assert_eq!(number_g(123456.0), "123456");
        assert_eq!(number_g(1234567.0), "1.23457e+06");
        assert_eq!(number_g(0.0001), "0.0001");
        assert_eq!(number_g(0.00001), "1e-05");
        assert_eq!(number_g(-1.5), "-1.5");
    }
}
