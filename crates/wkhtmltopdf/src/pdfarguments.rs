//! Registration of every wkhtmltopdf switch (122 long options). Ports
//! upstream `src/pdf/pdfarguments.cc`, with the 0.12.6 descriptions of
//! `--log-level` and `--default-header` (spec §9).
//!
//! Switch order, sections, scopes, `extended` and `qthack` flags follow the
//! upstream registration sequence exactly, including its accidents
//! (`--default-header` and `--viewport-size` inherit the QHACK flag of the
//! Outline section; `--page-offset` does not).

use wk_cli::arghandler::{
    Caller, ConstSetter, FloatSetter, IntSetter, LogLevelSetter, MapSetter, QStrSetter, SomeSetter,
    StringPairCreator, ValueType,
};
use wk_cli::{Action, CommandLineParserBase, Lens, Outcome};
use wk_settings::pdfsettings::{
    orientation_to_str, page_size_to_str, str_to_orientation, str_to_page_size, str_to_unit_real,
    unit_real_to_str,
};
use wk_settings::{ColorMode, LogLevel, Orientation, PageSize, PdfObject, PrinterMode, UnitReal};

use crate::pdfcommandlineparser::{GLOBAL, GlobalTarget, PAGE, TOC};

/// Lens on a field of the global target: `gl!(settings.dpi)`.
macro_rules! gl {
    ($($f:ident).+) => {
        Lens::global(|g: &GlobalTarget| &g.$($f).+, |g: &mut GlobalTarget| &mut g.$($f).+)
    };
}

/// Lens on a field of the object being parsed: `pg!(web.background)`.
macro_rules! pg {
    ($($f:ident).+) => {
        Lens::page(|p: &PdfObject| &p.$($f).+, |p: &mut PdfObject| &mut p.$($f).+)
    };
}

fn lossy(b: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(b)
}

/// `UnitRealTM`.
pub struct UnitRealTM;
impl ValueType for UnitRealTM {
    type T = UnitReal;
    fn str_to_t(val: &[u8]) -> (UnitReal, bool) {
        str_to_unit_real(&lossy(val))
    }
    fn t_to_str(t: &UnitReal) -> Option<String> {
        unit_real_to_str(t)
    }
}

/// `PageSizeTM`.
pub struct PageSizeTM;
impl ValueType for PageSizeTM {
    type T = PageSize;
    fn str_to_t(val: &[u8]) -> (PageSize, bool) {
        str_to_page_size(&lossy(val))
    }
    fn t_to_str(t: &PageSize) -> Option<String> {
        Some(page_size_to_str(*t).to_string())
    }
}

/// `OrientationTM`.
pub struct OrientationTM;
impl ValueType for OrientationTM {
    type T = Orientation;
    fn str_to_t(val: &[u8]) -> (Orientation, bool) {
        str_to_orientation(&lossy(val))
    }
    fn t_to_str(t: &Orientation) -> Option<String> {
        Some(orientation_to_str(*t).to_string())
    }
}

type UnitRealSetter = SomeSetter<GlobalTarget, PdfObject, UnitRealTM>;
type PageSizeSetter = SomeSetter<GlobalTarget, PdfObject, PageSizeTM>;
type OrientationSetter = SomeSetter<GlobalTarget, PdfObject, OrientationTM>;

/// `DefaultTocFunc`: `--dump-default-toc-xsl`.
fn default_toc(_: &[Vec<u8>], _: &mut GlobalTarget, _: &mut PdfObject) -> Outcome {
    Outcome::Action(Action::Custom(DUMP_DEFAULT_TOC_XSL))
}

/// Name of the custom action of `--dump-default-toc-xsl`.
pub const DUMP_DEFAULT_TOC_XSL: &str = "dump-default-toc-xsl";

/// `DefaultHeaderFunc`: writes the object header **and** the global top
/// margin.
fn default_header(_: &[Vec<u8>], g: &mut GlobalTarget, p: &mut PdfObject) -> Outcome {
    p.header.left = "[webpage]".into();
    p.header.right = "[page]/[topage]".into();
    p.header.line = true;
    g.settings.margin.top = str_to_unit_real("2cm").0;
    Outcome::Ok
}

/// Register every switch, in upstream order (`PdfCommandLineParser` ctor).
pub fn register(b: &mut CommandLineParserBase<GlobalTarget, PdfObject>) {
    b.section("Global Options", "");
    b.mode(GLOBAL);

    b.add_doc_args();

    b.extended(false);
    b.qthack(false);

    b.addarg(
        "quiet",
        Some(b'q'),
        "Be less verbose, maintained for backwards compatibility; Same as using --log-level none",
        ConstSetter::new(gl!(settings.log_level), LogLevel::None),
    );
    b.addarg(
        "log-level",
        None,
        "Set log level to: none, error, warn or info",
        LogLevelSetter::new(gl!(settings.log_level), "level"),
    );

    b.addarg(
        "no-collate",
        None,
        "Do not collate when printing multiple copies",
        ConstSetter::new(gl!(settings.collate), false),
    );
    b.addarg(
        "collate",
        None,
        "Collate when printing multiple copies",
        ConstSetter::new(gl!(settings.collate), true),
    );

    b.addarg(
        "copies",
        None,
        "Number of copies to print into the pdf file",
        IntSetter::new(gl!(settings.copies), "number"),
    );
    b.addarg(
        "orientation",
        Some(b'O'),
        "Set orientation to Landscape or Portrait",
        OrientationSetter::new(gl!(settings.orientation), "orientation"),
    );
    b.addarg(
        "page-size",
        Some(b's'),
        "Set paper size to: A4, Letter, etc.",
        PageSizeSetter::new(gl!(settings.size.page_size), "Size"),
    );

    b.addarg(
        "grayscale",
        Some(b'g'),
        "PDF will be generated in grayscale",
        ConstSetter::new(gl!(settings.color_mode), ColorMode::GrayScale),
    );

    b.addarg(
        "lowquality",
        Some(b'l'),
        "Generates lower quality pdf/ps. Useful to shrink the result document space",
        ConstSetter::new(gl!(settings.resolution), PrinterMode::ScreenResolution),
    );
    b.addarg(
        "title",
        None,
        "The title of the generated pdf file (The title of the first document is used if not specified)",
        QStrSetter::new(gl!(settings.document_title), "text"),
    );

    b.addarg(
        "read-args-from-stdin",
        None,
        "Read command line arguments from stdin",
        ConstSetter::new(gl!(read_args_from_stdin), true),
    );

    b.extended(true);
    b.qthack(false);
    b.addarg(
        "margin-bottom",
        Some(b'B'),
        "Set the page bottom margin",
        UnitRealSetter::new(gl!(settings.margin.bottom), "unitreal"),
    );
    b.addarg(
        "margin-left",
        Some(b'L'),
        "Set the page left margin",
        UnitRealSetter::new(gl!(settings.margin.left), "unitreal"),
    );
    b.addarg(
        "margin-right",
        Some(b'R'),
        "Set the page right margin",
        UnitRealSetter::new(gl!(settings.margin.right), "unitreal"),
    );
    b.addarg(
        "margin-top",
        Some(b'T'),
        "Set the page top margin",
        UnitRealSetter::new(gl!(settings.margin.top), "unitreal"),
    );

    b.addarg(
        "dpi",
        Some(b'd'),
        "Change the dpi explicitly (this has no effect on X11 based systems)",
        IntSetter::new(gl!(settings.dpi), "dpi"),
    );
    b.addarg("page-height", None, "Page height", UnitRealSetter::new(gl!(settings.size.height), "unitreal"));
    b.addarg("page-width", None, "Page width", UnitRealSetter::new(gl!(settings.size.width), "unitreal"));

    b.add_global_load_args(gl!(settings.load));

    b.extended(true);
    b.qthack(true);

    b.addarg(
        "image-quality",
        None,
        "When jpeg compressing images use this quality",
        IntSetter::new(gl!(settings.image_quality), "integer"),
    );
    b.addarg(
        "image-dpi",
        None,
        "When embedding images scale them down to this dpi",
        IntSetter::new(gl!(settings.image_dpi), "integer"),
    );
    b.addarg(
        "no-pdf-compression",
        None,
        "Do not use lossless compression on pdf objects",
        ConstSetter::new(gl!(settings.use_compression), false),
    );

    #[cfg(unix)]
    b.addarg(
        "use-xserver",
        None,
        "Use the X server (some plugins and other stuff might not work without X11)",
        ConstSetter::new(gl!(settings.use_graphics), true),
    );

    b.section("Outline Options", "");
    b.extended(true);
    b.qthack(true);
    b.addarg("outline", None, "Put an outline into the pdf", ConstSetter::new(gl!(settings.outline), true));
    b.addarg(
        "no-outline",
        None,
        "Do not put an outline into the pdf",
        ConstSetter::new(gl!(settings.outline), false),
    );
    b.addarg(
        "outline-depth",
        None,
        "Set the depth of the outline",
        IntSetter::new(gl!(settings.outline_depth), "level"),
    );
    b.addarg(
        "dump-outline",
        None,
        "Dump the outline to a file",
        QStrSetter::new(gl!(settings.dump_outline), "file"),
    );
    b.addarg(
        "dump-default-toc-xsl",
        None,
        "Dump the default TOC xsl style sheet to stdout",
        Caller::new(default_toc),
    );

    b.section("Page Options", "");
    b.mode(PAGE);
    b.addarg(
        "default-header",
        None,
        "Add a default header, with the name of the page to the left, and the page number to the right, this is short for: --header-left='[webpage]' --header-right='[page]/[toPage]' --top 2cm --header-line",
        Caller::new(default_header),
    );

    b.addarg(
        "viewport-size",
        None,
        "Set viewport size if you have custom scrollbars or css attribute overflow to emulate window size",
        QStrSetter::new(gl!(settings.viewport_size), ""),
    );
    b.add_web_args(pg!(web));
    b.extended(true);
    b.qthack(false);
    b.addarg("no-background", None, "Do not print background", ConstSetter::new(pg!(web.background), false));
    b.addarg("background", None, "Do print background", ConstSetter::new(pg!(web.background), true));

    b.extended(true);
    b.qthack(true);
    b.addarg(
        "include-in-outline",
        None,
        "Include the page in the table of contents and outlines",
        ConstSetter::new(pg!(include_in_outline), true),
    );
    b.addarg(
        "exclude-from-outline",
        None,
        "Do not include the page in the table of contents and outlines",
        ConstSetter::new(pg!(include_in_outline), false),
    );

    b.addarg(
        "disable-smart-shrinking",
        None,
        "Disable the intelligent shrinking strategy used by WebKit that makes the pixel/dpi ratio non-constant",
        ConstSetter::new(pg!(web.enable_intelligent_shrinking), false),
    );
    b.addarg(
        "enable-smart-shrinking",
        None,
        "Enable the intelligent shrinking strategy used by WebKit that makes the pixel/dpi ratio non-constant",
        ConstSetter::new(pg!(web.enable_intelligent_shrinking), true),
    );

    b.extended(false);
    b.qthack(true);
    b.addarg(
        "print-media-type",
        None,
        "Use print media-type instead of screen",
        ConstSetter::new(pg!(load.print_media_type), true),
    );
    b.addarg(
        "no-print-media-type",
        None,
        "Do not use print media-type instead of screen",
        ConstSetter::new(pg!(load.print_media_type), false),
    );

    b.extended(true);
    b.qthack(true);
    b.addarg(
        "enable-forms",
        None,
        "Turn HTML form fields into pdf form fields",
        ConstSetter::new(pg!(produce_forms), true),
    );
    b.addarg(
        "disable-forms",
        None,
        "Do not turn HTML form fields into pdf form fields",
        ConstSetter::new(pg!(produce_forms), false),
    );
    b.addarg(
        "disable-internal-links",
        None,
        "Do not make local links",
        ConstSetter::new(pg!(use_local_links), false),
    );
    b.addarg("enable-internal-links", None, "Make local links", ConstSetter::new(pg!(use_local_links), true));
    b.addarg(
        "disable-external-links",
        None,
        "Do not make links to remote web pages",
        ConstSetter::new(pg!(use_external_links), false),
    );
    b.addarg(
        "enable-external-links",
        None,
        "Make links to remote web pages",
        ConstSetter::new(pg!(use_external_links), true),
    );
    b.addarg(
        "resolve-relative-links",
        None,
        "Resolve relative external links into absolute links",
        ConstSetter::new(gl!(settings.resolve_relative_links), true),
    );
    b.addarg(
        "keep-relative-links",
        None,
        "Keep relative external links as relative external links",
        ConstSetter::new(gl!(settings.resolve_relative_links), false),
    );

    b.addarg(
        "enable-toc-back-links",
        None,
        "Link from section header to toc",
        ConstSetter::new(pg!(toc.back_links), true),
    );
    b.addarg(
        "disable-toc-back-links",
        None,
        "Do not link from section header to toc",
        ConstSetter::new(pg!(toc.back_links), false),
    );

    b.add_page_load_args(pg!(load));
    b.addarg(
        "page-offset",
        None,
        "Set the starting page number",
        IntSetter::with_def(gl!(settings.page_offset), "offset", true),
    );

    b.section("Headers And Footer Options", "");
    b.qthack(true);
    b.extended(true);
    b.addarg("footer-center", None, "Centered footer text", QStrSetter::new(pg!(footer.center), "text"));
    b.addarg(
        "footer-font-name",
        None,
        "Set footer font name",
        QStrSetter::new(pg!(footer.font_name), "name"),
    );
    b.addarg("footer-font-size", None, "Set footer font size", IntSetter::new(pg!(footer.font_size), "size"));
    b.addarg("footer-left", None, "Left aligned footer text", QStrSetter::new(pg!(footer.left), "text"));
    b.addarg("footer-line", None, "Display line above the footer", ConstSetter::new(pg!(footer.line), true));
    b.addarg(
        "no-footer-line",
        None,
        "Do not display line above the footer",
        ConstSetter::new(pg!(footer.line), false),
    );
    b.addarg("footer-right", None, "Right aligned footer text", QStrSetter::new(pg!(footer.right), "text"));
    b.addarg(
        "footer-spacing",
        None,
        "Spacing between footer and content in mm",
        FloatSetter::new(pg!(footer.spacing), "real"),
    );
    b.addarg("footer-html", None, "Adds a html footer", QStrSetter::new(pg!(footer.html_url), "url"));
    b.addarg("header-center", None, "Centered header text", QStrSetter::new(pg!(header.center), "text"));
    b.addarg(
        "header-font-name",
        None,
        "Set header font name",
        QStrSetter::new(pg!(header.font_name), "name"),
    );
    b.addarg("header-font-size", None, "Set header font size", IntSetter::new(pg!(header.font_size), "size"));
    b.addarg("header-left", None, "Left aligned header text", QStrSetter::new(pg!(header.left), "text"));
    b.addarg("header-line", None, "Display line below the header", ConstSetter::new(pg!(header.line), true));
    b.addarg(
        "no-header-line",
        None,
        "Do not display line below the header",
        ConstSetter::new(pg!(header.line), false),
    );
    b.addarg("header-right", None, "Right aligned header text", QStrSetter::new(pg!(header.right), "text"));
    b.addarg(
        "header-spacing",
        None,
        "Spacing between header and content in mm",
        FloatSetter::new(pg!(header.spacing), "real"),
    );
    b.addarg("header-html", None, "Adds a html header", QStrSetter::new(pg!(header.html_url), "url"));

    b.addarg(
        "replace",
        None,
        "Replace [name] with value in header and footer (repeatable)",
        MapSetter::<_, _, StringPairCreator>::new(pg!(replacements), "name", "value"),
    );

    b.section("TOC Options", "");
    b.mode(TOC);
    b.extended(true);
    b.qthack(true);
    b.addarg(
        "xsl-style-sheet",
        None,
        "Use the supplied xsl style sheet for printing the table of contents",
        QStrSetter::new(pg!(toc_xsl), "file"),
    );

    b.addarg(
        "toc-header-text",
        None,
        "The header text of the toc",
        QStrSetter::new(pg!(toc.caption_text), "text"),
    );
    b.addarg(
        "disable-toc-links",
        None,
        "Do not link from toc to sections",
        ConstSetter::new(pg!(toc.forward_links), false),
    );
    b.addarg(
        "disable-dotted-lines",
        None,
        "Do not use dotted lines in the toc",
        ConstSetter::new(pg!(toc.use_dotted_lines), false),
    );
    b.addarg(
        "toc-text-size-shrink",
        None,
        "For each level of headings in the toc the font is scaled by this factor",
        FloatSetter::new(pg!(toc.font_scale), "real"),
    );
    b.addarg(
        "toc-level-indentation",
        None,
        "For each level of headings in the toc indent by this length",
        QStrSetter::new(pg!(toc.indentation), "width"),
    );
}
