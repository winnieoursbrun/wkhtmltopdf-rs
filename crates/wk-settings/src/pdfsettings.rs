//! PDF conversion settings. Ports upstream `src/lib/pdfsettings.hh` and
//! `src/lib/pdfsettings.cc` (structures, defaults and string conversions; the
//! reflection part lives in `reflect.rs`).

use crate::loadsettings::{LoadGlobal, LoadPage};
use crate::logging::LogLevel;
use crate::qt;
use crate::websettings::Web;

/// `QPrinter::Unit` values reachable from `strToUnitReal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Millimeter,
    Point,
    Inch,
    Pica,
    Didot,
    Cicero,
    DevicePixel,
}

/// `settings::UnitReal` (`QPair<qreal, QPrinter::Unit>`). A value of `-1`
/// means "unset".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitReal {
    pub value: f64,
    pub unit: Unit,
}

impl UnitReal {
    pub const fn new(value: f64, unit: Unit) -> Self {
        UnitReal { value, unit }
    }

    /// The `UnitReal(-1, Millimeter)` default used for unset sizes/margins.
    pub const fn unset() -> Self {
        UnitReal::new(-1.0, Unit::Millimeter)
    }
}

/// `QPrinter::PageSize` values known to wkhtmltopdf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageSize {
    A0,
    A1,
    A2,
    A3,
    #[default]
    A4,
    A5,
    A6,
    A7,
    A8,
    A9,
    B0,
    B1,
    B2,
    B3,
    B4,
    B5,
    B6,
    B7,
    B8,
    B9,
    B10,
    C5E,
    Comm10E,
    DLE,
    Executive,
    Folio,
    Ledger,
    Legal,
    Letter,
    Tabloid,
}

/// `pageSizeMap()`, in `QMap` (sorted key) order.
pub const PAGE_SIZES: &[(&str, PageSize)] = &[
    ("A0", PageSize::A0),
    ("A1", PageSize::A1),
    ("A2", PageSize::A2),
    ("A3", PageSize::A3),
    ("A4", PageSize::A4),
    ("A5", PageSize::A5),
    ("A6", PageSize::A6),
    ("A7", PageSize::A7),
    ("A8", PageSize::A8),
    ("A9", PageSize::A9),
    ("B0", PageSize::B0),
    ("B1", PageSize::B1),
    ("B10", PageSize::B10),
    ("B2", PageSize::B2),
    ("B3", PageSize::B3),
    ("B4", PageSize::B4),
    ("B5", PageSize::B5),
    ("B6", PageSize::B6),
    ("B7", PageSize::B7),
    ("B8", PageSize::B8),
    ("B9", PageSize::B9),
    ("C5E", PageSize::C5E),
    ("Comm10E", PageSize::Comm10E),
    ("DLE", PageSize::DLE),
    ("Executive", PageSize::Executive),
    ("Folio", PageSize::Folio),
    ("Ledger", PageSize::Ledger),
    ("Legal", PageSize::Legal),
    ("Letter", PageSize::Letter),
    ("Tabloid", PageSize::Tabloid),
];

/// `QPrinter::Orientation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

/// `QPrinter::ColorMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    GrayScale,
    #[default]
    Color,
}

/// `QPrinter::PrinterMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrinterMode {
    ScreenResolution,
    PrinterResolution,
    #[default]
    HighResolution,
}

/// `settings::Margin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    pub top: UnitReal,
    pub right: UnitReal,
    pub bottom: UnitReal,
    pub left: UnitReal,
}

impl Default for Margin {
    fn default() -> Self {
        Margin {
            top: UnitReal::unset(),
            right: UnitReal::new(10.0, Unit::Millimeter),
            bottom: UnitReal::unset(),
            left: UnitReal::new(10.0, Unit::Millimeter),
        }
    }
}

/// `settings::Size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub page_size: PageSize,
    pub height: UnitReal,
    pub width: UnitReal,
}

impl Default for Size {
    fn default() -> Self {
        Size { page_size: PageSize::A4, height: UnitReal::unset(), width: UnitReal::unset() }
    }
}

/// `settings::TableOfContent`.
#[derive(Debug, Clone, PartialEq)]
pub struct TableOfContent {
    pub use_dotted_lines: bool,
    pub caption_text: String,
    pub forward_links: bool,
    pub back_links: bool,
    pub indentation: String,
    pub font_scale: f32,
}

impl Default for TableOfContent {
    fn default() -> Self {
        TableOfContent {
            use_dotted_lines: true,
            caption_text: "Table of Contents".into(),
            forward_links: true,
            back_links: false,
            indentation: "1em".into(),
            font_scale: 0.8,
        }
    }
}

/// `settings::PdfGlobal`.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfGlobal {
    pub size: Size,
    pub log_level: LogLevel,
    pub use_graphics: bool,
    pub resolve_relative_links: bool,
    pub orientation: Orientation,
    pub color_mode: ColorMode,
    pub resolution: PrinterMode,
    pub dpi: i32,
    pub page_offset: i32,
    pub copies: i32,
    pub collate: bool,
    pub outline: bool,
    pub outline_depth: i32,
    pub dump_outline: String,
    pub out: String,
    pub document_title: String,
    pub use_compression: bool,
    pub margin: Margin,
    pub viewport_size: String,
    pub image_dpi: i32,
    pub image_quality: i32,
    pub load: LoadGlobal,
}

impl Default for PdfGlobal {
    fn default() -> Self {
        PdfGlobal {
            size: Size::default(),
            log_level: LogLevel::Info,
            use_graphics: false,
            resolve_relative_links: true,
            orientation: Orientation::Portrait,
            color_mode: ColorMode::Color,
            resolution: PrinterMode::HighResolution,
            dpi: 96,
            page_offset: 0,
            copies: 1,
            collate: true,
            outline: true,
            outline_depth: 4,
            dump_outline: String::new(),
            out: String::new(),
            document_title: String::new(),
            use_compression: true,
            margin: Margin::default(),
            viewport_size: String::new(),
            image_dpi: 600,
            image_quality: 94,
            load: LoadGlobal::default(),
        }
    }
}

/// `settings::HeaderFooter`.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFooter {
    pub font_size: i32,
    pub font_name: String,
    pub left: String,
    pub right: String,
    pub center: String,
    pub line: bool,
    pub html_url: String,
    pub spacing: f32,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        HeaderFooter {
            font_size: 12,
            font_name: "Arial".into(),
            left: String::new(),
            right: String::new(),
            center: String::new(),
            line: false,
            html_url: String::new(),
            spacing: 0.0,
        }
    }
}

/// `settings::PdfObject`: one page, cover or toc object.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfObject {
    pub toc: TableOfContent,
    pub page: String,
    pub header: HeaderFooter,
    pub footer: HeaderFooter,
    pub use_external_links: bool,
    pub use_local_links: bool,
    pub replacements: Vec<(String, String)>,
    pub produce_forms: bool,
    pub load: LoadPage,
    pub web: Web,
    pub include_in_outline: bool,
    pub pages_count: bool,
    pub is_table_of_content: bool,
    pub toc_xsl: String,
}

impl Default for PdfObject {
    fn default() -> Self {
        PdfObject {
            toc: TableOfContent::default(),
            page: String::new(),
            header: HeaderFooter::default(),
            footer: HeaderFooter::default(),
            use_external_links: true,
            use_local_links: true,
            replacements: Vec::new(),
            produce_forms: false,
            load: LoadPage::default(),
            web: Web::default(),
            include_in_outline: true,
            pages_count: true,
            is_table_of_content: false,
            toc_xsl: String::new(),
        }
    }
}

/// `strToPageSize`: case-insensitive match; unknown sizes give `A4` and
/// `ok == false`.
pub fn str_to_page_size(s: &str) -> (PageSize, bool) {
    for (k, v) in PAGE_SIZES {
        if k.eq_ignore_ascii_case(s) {
            return (*v, true);
        }
    }
    (PageSize::A4, false)
}

/// `pageSizeToStr`: canonical name.
pub fn page_size_to_str(ps: PageSize) -> &'static str {
    PAGE_SIZES.iter().find(|(_, v)| *v == ps).map(|(k, _)| *k).unwrap_or("")
}

/// `strToOrientation`: case-insensitive `Landscape|Portrait`.
pub fn str_to_orientation(s: &str) -> (Orientation, bool) {
    if s.eq_ignore_ascii_case("Landscape") {
        (Orientation::Landscape, true)
    } else if s.eq_ignore_ascii_case("Portrait") {
        (Orientation::Portrait, true)
    } else {
        (Orientation::Portrait, false)
    }
}

/// `orientationToStr`.
pub fn orientation_to_str(o: Orientation) -> &'static str {
    match o {
        Orientation::Landscape => "Landscape",
        Orientation::Portrait => "Portrait",
    }
}

/// `strToUnitReal`: `[0-9]*[.]?[0-9]*` followed by a case-insensitive unit.
/// No sign, no exponent; `""` and `"."` are invalid.
pub fn str_to_unit_real(o: &str) -> (UnitReal, bool) {
    let b = o.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let num = &o[..i];
    let suffix = &o[i..];
    let is = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(suffix));
    let (unit, scale) = if is(&["", "mm", "millimeter"]) {
        (Unit::Millimeter, 1.0)
    } else if is(&["cm", "centimeter"]) {
        (Unit::Millimeter, 10.0)
    } else if is(&["m", "meter"]) {
        (Unit::Millimeter, 1000.0)
    } else if is(&["didot"]) {
        (Unit::Didot, 1.0)
    } else if is(&["inch", "in"]) {
        (Unit::Inch, 1.0)
    } else if is(&["pica", "pc"]) {
        (Unit::Pica, 1.0)
    } else if is(&["cicero"]) {
        (Unit::Cicero, 1.0)
    } else if is(&["pixel", "px"]) {
        (Unit::DevicePixel, 1.0)
    } else if is(&["point", "pt"]) {
        (Unit::Point, 1.0)
    } else {
        let v = qt::to_double(num).unwrap_or(0.0);
        return (UnitReal::new(v, Unit::Millimeter), false);
    };
    match qt::to_double(num) {
        Some(v) => (UnitReal::new(v * scale, unit), true),
        None => (UnitReal::new(0.0, unit), false),
    }
}

/// `unitRealToStr`: `None` for unset (`-1`) values and for Cicero.
pub fn unit_real_to_str(ur: &UnitReal) -> Option<String> {
    if ur.value == -1.0 {
        return None;
    }
    let c = match ur.unit {
        Unit::Didot => "didot",
        Unit::Inch => "in",
        Unit::Pica => "pica",
        Unit::DevicePixel => "px",
        Unit::Point => "pt",
        Unit::Millimeter => "mm",
        Unit::Cicero => return None,
    };
    Some(format!("{}{}", qt::number_g(ur.value), c))
}

/// `strToPrinterMode`: case-insensitive `screen|printer|high`.
pub fn str_to_printer_mode(s: &str) -> (PrinterMode, bool) {
    if s.eq_ignore_ascii_case("screen") {
        (PrinterMode::ScreenResolution, true)
    } else if s.eq_ignore_ascii_case("printer") {
        (PrinterMode::PrinterResolution, true)
    } else if s.eq_ignore_ascii_case("high") {
        (PrinterMode::HighResolution, true)
    } else {
        (PrinterMode::HighResolution, false)
    }
}

/// `printerModeToStr`.
pub fn printer_mode_to_str(m: PrinterMode) -> &'static str {
    match m {
        PrinterMode::ScreenResolution => "screen",
        PrinterMode::PrinterResolution => "printer",
        PrinterMode::HighResolution => "high",
    }
}

/// `strToColorMode`: case-insensitive `color|grayscale`.
pub fn str_to_color_mode(s: &str) -> (ColorMode, bool) {
    if s.eq_ignore_ascii_case("color") {
        (ColorMode::Color, true)
    } else if s.eq_ignore_ascii_case("grayscale") {
        (ColorMode::GrayScale, true)
    } else {
        (ColorMode::Color, false)
    }
}

/// `colorModeToStr`.
pub fn color_mode_to_str(m: ColorMode) -> &'static str {
    match m {
        ColorMode::Color => "color",
        ColorMode::GrayScale => "grayscale",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ur(s: &str) -> (UnitReal, bool) {
        str_to_unit_real(s)
    }

    #[test]
    fn unitreal_units() {
        assert_eq!(ur("10"), (UnitReal::new(10.0, Unit::Millimeter), true));
        assert_eq!(ur("2cm"), (UnitReal::new(20.0, Unit::Millimeter), true));
        assert_eq!(ur("1M"), (UnitReal::new(1000.0, Unit::Millimeter), true));
        assert_eq!(ur("0.75in"), (UnitReal::new(0.75, Unit::Inch), true));
        assert_eq!(ur("3PT"), (UnitReal::new(3.0, Unit::Point), true));
        assert_eq!(ur(".5px"), (UnitReal::new(0.5, Unit::DevicePixel), true));
        assert_eq!(ur("5.pica"), (UnitReal::new(5.0, Unit::Pica), true));
        assert_eq!(ur("1cicero").0.unit, Unit::Cicero);
        assert_eq!(ur("1didot").0.unit, Unit::Didot);
    }

    #[test]
    fn unitreal_has_no_sign_and_no_exponent() {
        for s in ["-1cm", "1e2mm", "", ".", "cm", "1.5.5", "1 cm", "1km"] {
            assert!(!ur(s).1, "{s:?} should be invalid");
        }
    }

    #[test]
    fn unitreal_display() {
        assert_eq!(unit_real_to_str(&UnitReal::new(10.0, Unit::Millimeter)).as_deref(), Some("10mm"));
        assert_eq!(unit_real_to_str(&UnitReal::new(0.75, Unit::Inch)).as_deref(), Some("0.75in"));
        assert_eq!(unit_real_to_str(&UnitReal::unset()), None);
        assert_eq!(unit_real_to_str(&UnitReal::new(1.0, Unit::Cicero)), None);
    }

    #[test]
    fn page_size_case_insensitive_and_canonical() {
        assert_eq!(str_to_page_size("letter"), (PageSize::Letter, true));
        assert_eq!(str_to_page_size("comm10e"), (PageSize::Comm10E, true));
        assert_eq!(str_to_page_size("A11"), (PageSize::A4, false));
        assert_eq!(page_size_to_str(PageSize::B10), "B10");
    }

    #[test]
    fn orientation_values() {
        assert_eq!(str_to_orientation("landscape"), (Orientation::Landscape, true));
        assert_eq!(str_to_orientation("PORTRAIT"), (Orientation::Portrait, true));
        assert!(!str_to_orientation("sideways").1);
    }

    #[test]
    fn defaults_match_upstream() {
        let g = PdfGlobal::default();
        assert_eq!(g.dpi, 96);
        assert_eq!(g.margin.left, UnitReal::new(10.0, Unit::Millimeter));
        assert_eq!(g.margin.top, UnitReal::unset());
        assert_eq!(g.image_dpi, 600);
        assert_eq!(g.image_quality, 94);
        let o = PdfObject::default();
        assert!(o.load.block_local_file_access);
        assert_eq!(o.load.jsdelay, 200);
        assert_eq!(o.toc.caption_text, "Table of Contents");
        assert_eq!(o.header.font_name, "Arial");
        assert_eq!(o.web.minimum_font_size, -1);
    }
}
