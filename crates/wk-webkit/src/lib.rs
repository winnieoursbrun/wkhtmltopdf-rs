//! The web engine facade. Stands in for QtWebKit (`QWebPage`, `QWebFrame`,
//! `QWebSettings`, `QWebPrinter`) as used by wkhtmltopdf: parse, load
//! sub-resources, style, lay out for printing, and paint into a display
//! list. Built on blitz-dom/blitz-html/blitz-paint 0.3 (html5ever, stylo,
//! taffy, parley) with the vendored patches described in DESIGN.md.

pub mod encoding;
pub mod net;
pub mod page;
pub mod paint;
mod parser;
pub mod profile;
mod scripting;

pub use blitz_paint::run_text::RunText;
pub use net::Fetcher;
pub use page::{
    Anchors, FormElement, Geometry, Heading, LinkBox, RenderedPage, TableInfo, WebPage, WebSettings,
};
pub use paint::{Command, DisplayList, Recorder, RunTexts};

/// Work every conversion needs whatever its input, done ahead of time on the calling
/// thread (the binary runs it on a worker while the input is read and parsed): the
/// user-agent stylesheet's parse and the font scan. A conversion that needs them meanwhile
/// waits for them.
pub fn warm_up() {
    rayon::join(
        || blitz_dom::preparse_user_agent_stylesheet(blitz_dom::DEFAULT_CSS),
        // The font collection of the conversion (`wk_fonts::default_font_context`).
        || drop(wk_fonts::default_font_context()),
    );
}

/// Let a large document use `threads` threads to style and build its text layouts (see
/// `blitz_dom::set_parallelism`; 1 keeps everything on the calling thread). The output does
/// not depend on it.
pub fn set_parallelism(threads: usize) {
    blitz_dom::set_parallelism(threads);
}

/// `WKHTMLTOPDF_DEBUG_TIMING`: print the elapsed time at a few pipeline steps to stderr.
pub fn timing(label: &str) {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    if let Some(t0) =
        START.get_or_init(|| std::env::var_os("WKHTMLTOPDF_DEBUG_TIMING").map(|_| Instant::now()))
    {
        eprintln!("timing {:8.2} ms {label}", t0.elapsed().as_secs_f64() * 1000.0);
    }
}
