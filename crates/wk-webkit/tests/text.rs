//! Text rendering as QtWebKit 2.2 (wkhtmltopdf 0.12.6) does it, checked on the display list:
//! which face each run is painted with, and where its glyphs go. The expected values come from
//! the reference PDFs of `tests/fidelity` (made with the real 0.12.6 binary).

use anyrender::recording::RenderCommand;
use url::Url;
use wk_webkit::{Fetcher, WebPage, WebSettings};

/// Serves nothing.
struct NoNet;
impl Fetcher for NoNet {
    fn create(&mut self, _: &Url) -> usize {
        0
    }
    fn finish(&mut self, _: usize) -> Option<Vec<u8>> {
        None
    }
}

/// A painted glyph run.
#[derive(Debug)]
struct Run {
    text: String,
    /// File name of the embedded face it is painted with.
    face: &'static str,
    /// Glyph x positions, in CSS px.
    xs: Vec<f32>,
}

fn runs(html: &str) -> Vec<Run> {
    let fonts = wk_fonts::font_context(&wk_fonts::FontOptions { system_fonts: false, extra_dirs: Vec::new() });
    let url = Url::parse("file:///tmp/page.html").unwrap();
    let mut page = WebPage::load(html, url, WebSettings::default(), Some(fonts), &mut NoNet);
    let rendered = page.render(612.0 - 108.0, 792.0 - 108.0);
    let texts: Vec<_> = rendered.recorder.texts.iter().map(|(i, t)| (*i, t.text.clone())).collect();
    let mut out = Vec::new();
    for (i, cmd) in rendered.recorder.commands.iter().enumerate() {
        let RenderCommand::GlyphRun(run) = &*cmd.get() else { continue };
        let len = run.font_data.data.data().len();
        let face = wk_fonts::EMBEDDED_FONTS
            .iter()
            .find(|(_, bytes)| bytes.len() == len)
            .map(|(name, _)| *name)
            .unwrap_or("?");
        let text = texts.iter().find(|(j, _)| *j == i).map(|(_, t)| t.clone()).unwrap_or_default();
        let xs = run.glyphs.iter().map(|g| g.x).collect();
        out.push(Run { text, face, xs });
    }
    out
}

/// An unknown family without a generic fallback (`Arial`, `Times`, `NoSuchFont`) resolves to
/// the last-resort face: after a bold run, the rest of the paragraph is regular again.
#[test]
fn unknown_family_after_bold_run_is_regular() {
    let r = runs(r#"<p style="font-family: Arial"><b>Arial</b>: regular text</p>"#);
    let bold: Vec<_> = r.iter().filter(|r| r.text.contains("Arial")).collect();
    let regular: Vec<_> = r.iter().filter(|r| r.text.contains("regular")).collect();
    assert!(!bold.is_empty() && !regular.is_empty(), "{r:?}");
    assert!(bold.iter().all(|r| r.face == "DejaVuSans-Bold.ttf"), "{r:?}");
    assert!(regular.iter().all(|r| r.face == "DejaVuSans.ttf"), "{r:?}");
}
