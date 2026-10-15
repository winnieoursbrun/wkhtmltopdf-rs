//! Library side of the `wkhtmltopdf` binary (ports upstream `src/pdf/*`):
//! the command line parser with its object grammar, the documentation parts,
//! and the program flow. Exposed as a library for tests.

// The allocator's size classes are those of 64-bit dlmalloc.
#[cfg(all(unix, target_pointer_width = "64"))]
pub mod alloc;
pub mod app;
pub mod convert;
pub mod pdfarguments;
pub mod pdfcommandlineparser;
pub mod pdfdocparts;
pub mod stdinargs;
pub use wk_converter::tocstylesheet;
pub use wk_loader::urlguess;

pub use app::run;
pub use pdfcommandlineparser::{Exit, GlobalTarget, PdfCommandLineParser};
