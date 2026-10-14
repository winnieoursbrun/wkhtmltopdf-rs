//! Printing: page breaking and PDF output. Stands in for the `QPrinter` /
//! patched `QPdfEngine` usage of upstream `src/lib/pdfconverter.cc` and the
//! printing code of the patched QtWebKit (`QWebPrinter`).

pub mod paginate;
pub mod pdf;

pub use paginate::{PageSlice, paginate};
pub use pdf::{DocInfo, DocPart, LinkTarget, OutlineNode, PageSetup, PdfLink, write_pdf};
