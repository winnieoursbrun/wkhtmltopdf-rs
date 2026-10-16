//! Process helpers. Ports `handleError` from upstream
//! `src/lib/utilities.cc` (spec §4.3) and the unit/paper conversions of
//! `QPrinter` that wkhtmltopdf relies on.

use wk_loader::NetworkError;
use wk_settings::{Orientation, PageSize, PdfGlobal, Unit, UnitReal};

/// `handleError(success, errorCode)`: the exit code, plus the `Exit with
/// code …` line that is printed whatever the log level.
pub fn handle_error(success: bool, error_code: i32) -> (i32, Option<String>) {
    if error_code != 0 {
        let c = match error_code {
            404 => 2,
            401 => 3,
            _ => 1,
        };
        let line = if error_code < 1000 {
            let text = match error_code {
                400 => "Bad Request",
                401 => "Unauthorized",
                402 => "Payment Required",
                403 => "Forbidden",
                404 => "Page not found",
                405 => "Method Not Allowed",
                500 => "Internal Server Error",
                501 => "Not Implemented",
                503 => "Service Unavailable",
                505 => "HTTP Version Not Supported",
                _ => "",
            };
            format!("Exit with code {c} due to http error: {error_code} {text}\n")
        } else {
            let key = NetworkError::from_code(error_code - 1000).map(NetworkError::key).unwrap_or("");
            format!("Exit with code {c} due to network error: {key}\n")
        };
        return (c, Some(line));
    }
    if !success {
        return (1, Some("Exit with code 1, due to unknown error.\n".into()));
    }
    (0, None)
}

/// Points per unit (`QPrinter::Unit`). Device pixels depend on `dpi`.
pub fn points_per_unit(unit: Unit, dpi: i32) -> f64 {
    match unit {
        Unit::Millimeter => 72.0 / 25.4,
        Unit::Point => 1.0,
        Unit::Inch => 72.0,
        Unit::Pica => 12.0,
        Unit::Didot => 0.375 * 72.0 / 25.4,
        Unit::Cicero => 4.5 * 72.0 / 25.4,
        Unit::DevicePixel => 72.0 / f64::from(dpi.max(1)),
    }
}

/// A `UnitReal` in points.
pub fn to_points(v: UnitReal, dpi: i32) -> f64 {
    v.value * points_per_unit(v.unit, dpi)
}

/// Paper sizes of Qt 4's PDF engine, in points (`QPdf::paperSize`), portrait.
pub fn paper_points(ps: PageSize) -> (f64, f64) {
    use PageSize::*;
    let (w, h) = match ps {
        A0 => (2384, 3370),
        A1 => (1684, 2384),
        A2 => (1191, 1684),
        A3 => (842, 1191),
        A4 => (595, 842),
        A5 => (420, 595),
        A6 => (298, 420),
        A7 => (210, 298),
        A8 => (147, 210),
        A9 => (105, 147),
        B0 => (2835, 4008),
        B1 => (2004, 2835),
        B2 => (1417, 2004),
        B3 => (1001, 1417),
        B4 => (709, 1001),
        B5 => (499, 709),
        B6 => (354, 499),
        B7 => (249, 354),
        B8 => (176, 249),
        B9 => (125, 176),
        B10 => (88, 125),
        C5E => (459, 649),
        Comm10E => (297, 684),
        DLE => (312, 624),
        Executive => (522, 756),
        Folio => (595, 935),
        Ledger => (1224, 792),
        Legal => (612, 1008),
        Letter => (612, 792),
        Tabloid => (792, 1224),
    };
    (f64::from(w), f64::from(h))
}

/// The paper of the output in points, orientation applied: custom
/// `--page-width`/`--page-height` (both needed, in the height's unit) or
/// `--page-size`.
pub fn paper_size(g: &PdfGlobal) -> (f64, f64) {
    let (w, h) = if g.size.height.value != -1.0 && g.size.width.value != -1.0 {
        // Exact: QPrinter's rounding happens in the converter's `Paper`.
        let ppu = points_per_unit(g.size.height.unit, g.dpi);
        (g.size.width.value * ppu, g.size.height.value * ppu)
    } else {
        paper_points(g.size.page_size)
    };
    match g.orientation {
        Orientation::Portrait => (w, h),
        Orientation::Landscape => (h, w),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(handle_error(true, 0), (0, None));
        assert_eq!(handle_error(false, 0).0, 1);
        assert_eq!(
            handle_error(true, 1003).1.unwrap(),
            "Exit with code 1 due to network error: HostNotFoundError\n"
        );
        assert_eq!(
            handle_error(true, 404),
            (2, Some("Exit with code 2 due to http error: 404 Page not found\n".into()))
        );
        assert_eq!(handle_error(true, 401).0, 3);
        assert_eq!(handle_error(true, 410).1.unwrap(), "Exit with code 1 due to http error: 410 \n");
    }

    #[test]
    fn letter_and_landscape() {
        let mut g = PdfGlobal::default();
        g.size.page_size = PageSize::Letter;
        assert_eq!(paper_size(&g), (612.0, 792.0));
        g.orientation = Orientation::Landscape;
        assert_eq!(paper_size(&g), (792.0, 612.0));
    }
}
