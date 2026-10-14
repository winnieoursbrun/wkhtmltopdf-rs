//! wkhtmltopdf-rs: a hook to rewrite author CSS before stylo parses it.
//!
//! The embedder installs a function (wkhtmltopdf-rs uses it for its
//! "WebKit-2012 profile": properties QtWebKit 2.2 did not know are dropped,
//! `-webkit-` prefixed ones are mapped onto the standard ones). It is applied
//! to `<style>` elements, linked and imported stylesheets and `style`
//! attributes; user-agent sheets are not rewritten.

use std::borrow::Cow;
use std::sync::OnceLock;

/// What is being rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssSource {
    /// A whole stylesheet.
    Sheet,
    /// The declarations of a `style` attribute.
    Declarations,
}

/// The installed rewrite function.
pub type CssRewriter = fn(&str, CssSource) -> String;

static REWRITER: OnceLock<CssRewriter> = OnceLock::new();

/// Install the rewrite function (once per process; later calls are ignored).
pub fn set_css_rewriter(f: CssRewriter) {
    let _ = REWRITER.set(f);
}

/// `css` rewritten by the installed function, or unchanged.
pub fn rewrite(css: &str, source: CssSource) -> Cow<'_, str> {
    match REWRITER.get() {
        Some(f) => Cow::Owned(f(css, source)),
        None => Cow::Borrowed(css),
    }
}
