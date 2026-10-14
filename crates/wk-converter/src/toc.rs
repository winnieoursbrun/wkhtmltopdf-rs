//! Table of contents objects. Ports the TOC part of upstream
//! `src/lib/pdfconverter.cc` (`loadTocs`): the outline XML and the style
//! sheet (default from `tocstylesheet.rs`, or `--xsl-style-sheet`) are
//! transformed into the HTML page of the TOC object (spec §3.1 step 4,
//! §3.5).
//!
//! Like upstream, a style sheet that fails to compile or run prints
//! QtXmlPatterns' message on stderr (with the executable as base URI, as
//! `QXmlQuery::setQuery(QIODevice*)` does) and produces an empty page, and
//! the conversion goes on.

use std::path::PathBuf;

use wk_settings::PdfObject;

use crate::tocstylesheet::dump_default_toc_style_sheet;
use crate::xslt::Stylesheet;

/// The `file://` URL of the running executable: the base URI QtXmlPatterns
/// reports errors against.
pub fn executable_url() -> String {
    let exe = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok()).unwrap_or_default();
    url::Url::from_file_path(&exe)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{}", exe.display()))
}

/// Why a TOC could not be built (the messages of `loadTocs`).
#[derive(Debug, Clone, PartialEq)]
pub enum TocError {
    /// `Could not read the TOC XSL`.
    ReadXsl,
}

/// The style sheet of a TOC object: `--xsl-style-sheet` or the default.
pub fn style_sheet(ps: &PdfObject) -> Result<String, TocError> {
    if ps.toc_xsl.is_empty() {
        return Ok(dump_default_toc_style_sheet(&ps.toc));
    }
    let bytes = std::fs::read(&ps.toc_xsl).map_err(|_| TocError::ReadXsl)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Run the transformation. On error, the message QtXmlPatterns prints is
/// returned with an empty document.
pub fn transform(xsl: &str, outline_xml: &str) -> (String, Option<String>) {
    let r = Stylesheet::parse(xsl).and_then(|s| s.transform(outline_xml));
    match r {
        Ok(html) => (html, None),
        Err(e) => (String::new(), Some(e.report(&executable_url()))),
    }
}

/// Temporary files of the TOC (`TempFile`), removed on drop.
#[derive(Default)]
pub struct TempFiles(pub Vec<PathBuf>);

impl TempFiles {
    /// Write `data` to a new `wktemp-<uuid><ext>` file.
    pub fn write(&mut self, ext: &str, data: &[u8]) -> Option<PathBuf> {
        let p = wk_loader::temp_path(ext);
        std::fs::write(&p, data).ok()?;
        self.0.push(p.clone());
        Some(p)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}
