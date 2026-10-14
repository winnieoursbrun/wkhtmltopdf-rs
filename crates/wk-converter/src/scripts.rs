//! Script execution hook of the pipeline. Stands in for the moment upstream
//! QtWebKit runs a page's scripts (between `loadFinished` and printing in
//! `src/lib/multipageloader.cc` / `pdfconverter.cc`).
//!
//! Every page the converter loads goes through [`run_scripts`] before it is
//! laid out: documents, TOC pages and every per-page copy of a
//! `--header-html`/`--footer-html` page (whose canonical `subst()` script
//! reads the variables from `location.search`). JavaScript is phase P4: the
//! hook does nothing yet, so header/footer HTML is rendered statically.

use wk_webkit::WebPage;

/// What a loaded page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Document,
    Toc,
    HeaderFooter,
}

/// Run the page's scripts (P4). No-op for now.
pub fn run_scripts(_page: &mut WebPage, _kind: PageKind) {}
