//! Printing: page breaking and PDF output. Stands in for the `QPrinter` /
//! patched `QPdfEngine` usage of upstream `src/lib/pdfconverter.cc` and the
//! printing code of the patched QtWebKit (`QWebPrinter`).

pub mod acroform;
pub mod paginate;
pub mod pdf;

pub use paginate::{PageSlice, paginate};
pub use pdf::{
    Anchor, Band, DocInfo, FontFace, FormField, LineItem, LinkTarget, OutPage, OutlineNode, PageSetup,
    PdfLink, PdfOptions, TextItem, write_pdf,
};
