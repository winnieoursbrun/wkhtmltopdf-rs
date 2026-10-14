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
pub use paint::{Command, DisplayList, Recorder};
