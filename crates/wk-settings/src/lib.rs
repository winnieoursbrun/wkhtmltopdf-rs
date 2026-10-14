//! Settings model of wkhtmltopdf: `PdfGlobal`, `PdfObject`, `LoadPage`,
//! `Web`, `HeaderFooter`, `TableOfContent`, with upstream defaults, the string
//! conversions used by the command line, and the dotted-name reflection used
//! by the C bindings.
//!
//! Ports upstream `src/lib/pdfsettings.*`, `src/lib/loadsettings.*`,
//! `src/lib/websettings.*`, `src/lib/logging.*` and `src/lib/reflect.*`.

pub mod loadsettings;
pub mod logging;
pub mod pdfsettings;
pub mod qt;
pub mod reflect;
pub mod websettings;

pub use loadsettings::{LoadErrorHandling, LoadGlobal, LoadPage, PostItem, Proxy, ProxyType};
pub use logging::LogLevel;
pub use pdfsettings::{
    ColorMode, HeaderFooter, Margin, Orientation, PageSize, PdfGlobal, PdfObject, PrinterMode, Size,
    TableOfContent, Unit, UnitReal,
};
pub use reflect::Reflect;
pub use websettings::Web;
