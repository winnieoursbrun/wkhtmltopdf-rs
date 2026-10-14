//! Which loader a page belongs to, for its scripts. Upstream QtWebKit runs
//! every page's scripts during its load (`src/lib/multipageloader.cc`):
//! documents, TOC pages, the header/footer measuring load and every per-page
//! copy of a `--header-html`/`--footer-html` page (whose canonical `subst()`
//! script reads the variables from `location.search`). Here that happens
//! inside `wk_webkit::WebPage::load`; `--run-script` and `--window-status`
//! only apply to the main loader (documents).

/// What a loaded page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Document,
    Toc,
    HeaderFooter,
}

impl PageKind {
    /// Whether the page belongs to the main pages loader
    /// (`isMainLoader`: `--run-script` and `--window-status` apply).
    pub fn is_main_loader(self) -> bool {
        self == PageKind::Document
    }
}
