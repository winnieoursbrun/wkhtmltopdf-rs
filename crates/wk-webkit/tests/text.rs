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

/// Glyphs are painted at their unrounded (26.6 truncated) advances from the start of the text
/// box, although the line breaker measured each "spaces + word" segment as whole pixels: in
/// DejaVu Sans 14px, `k` is 8.09375px and the space 4.4375px, so the second word starts at
/// 20.625px (not at round(16.1875) + 4.4375 = 20.4375px).
#[test]
fn glyphs_are_painted_at_unrounded_advances() {
    let r = runs(r#"<p style="font: 14px sans-serif">kk kk</p>"#);
    let run = r.iter().find(|r| r.text.starts_with("kk")).expect("run");
    let x0 = run.xs[0];
    let xs: Vec<f32> = run.xs.iter().map(|x| x - x0).collect();
    let k = 8.09375f32;
    let expected = [0.0, k, 2.0 * k, 2.0 * k + 4.4375, 3.0 * k + 4.4375];
    assert_eq!(xs.len(), expected.len(), "{xs:?}");
    for (got, want) in xs.iter().zip(expected) {
        assert!((got - want).abs() < 0.01, "{xs:?} != {expected:?}");
    }
}

/// A right-aligned text node is placed by its preferred width, each word and each space
/// rounded on its own (`RenderText::maxLogicalWidth`): `Paris, 1 October 2026` in DejaVu Sans
/// 14px is 38 + 4 + 9 + 4 + 56 + 4 + 36 = 151px (its glyphs add up to 152.19px), so in a 300px
/// block it starts at 149px (reference: doc-letter; glyph x are relative to the block).
#[test]
fn right_aligned_text_uses_the_preferred_width() {
    let r = runs(r#"<p style="font: 14px sans-serif; text-align: right; width: 300px">Paris, 1 October 2026</p>"#);
    let run = r.iter().find(|r| r.text.starts_with("Paris")).expect("run");
    assert!((run.xs[0] - 149.0).abs() < 0.01, "{:?}", run.xs);
}

/// List markers in WebKit 2012's numbering systems, `start`, `value` and `type` (reference:
/// lists-types).
#[test]
fn list_marker_texts() {
    let r = runs(
        r#"<ol style="list-style-type: lower-roman" start="8"><li>a</li><li>b</li></ol>
        <ol style="list-style-type: decimal-leading-zero"><li>a</li></ol>
        <ol style="list-style-type: lower-greek"><li>a</li><li>b</li></ol>
        <ol style="list-style-type: armenian"><li>a</li></ol>
        <ol style="list-style-type: georgian"><li>a</li></ol>
        <ol start="5"><li>a</li><li value="20">b</li><li>c</li></ol>
        <ol type="A"><li>a</li><li>b</li></ol><ol type="i"><li>a</li></ol>"#,
    );
    let markers: Vec<&str> = r.iter().map(|r| r.text.trim_end()).filter(|t| t.ends_with('.')).collect();
    assert_eq!(
        markers,
        ["viii.", "ix.", "01.", "α.", "β.", "Ա.", "ა.", "5.", "20.", "21.", "A.", "B.", "i."],
        "{r:?}"
    );
}

/// Letter-spaced text goes through Qt's complex path, which kerns: in DejaVu Sans 13px with
/// `letter-spacing: 1px`, `P` is followed by `a` 7.828px later (8.828px unkerned), as in the
/// reference typo-spacing.
#[test]
fn letter_spaced_text_is_kerned() {
    let r = runs(r#"<p style="font: 13px sans-serif; letter-spacing: 1px">Pa</p>"#);
    let run = r.iter().find(|r| r.text.starts_with("Pa")).expect("run");
    assert!((run.xs[1] - run.xs[0] - 7.828).abs() < 0.01, "{r:?}");
}

/// `counter()` and nested quotes in generated content, resolved in document order (reference:
/// css-generated).
#[test]
fn counters_and_quotes() {
    let r = runs(
        r#"<style>body { counter-reset: ch } h2 { counter-increment: ch; counter-reset: sec }
        h3 { counter-increment: sec } h2:before { content: "C" counter(ch) " " }
        h3:before { content: counter(ch) "." counter(sec, lower-roman) " " }</style>
        <h2>a</h2><h3>b</h3><h3>c</h3><h2>d</h2><h3>e</h3><p><q>x <q>y</q></q></p>"#,
    );
    let text: String = r.iter().map(|r| r.text.as_str()).collect();
    assert!(text.contains("C1 a1.i b1.ii cC2 d2.i e"), "{text:?}");
    assert!(text.contains("\"x 'y'\""), "{text:?}");
}

/// A tab is drawn as a space, also in preformatted text (reference: typo-whitespace).
#[test]
fn tab_is_a_space() {
    let r = runs("<pre style=\"font: 14px sans-serif\">\tx</pre>");
    let run = r.iter().find(|r| r.text.contains('x')).expect("run");
    assert_eq!(run.text, " x", "{r:?}");
    assert!((run.xs[1] - run.xs[0] - 4.4375).abs() < 0.01, "{r:?}");
}

/// Text in an `@font-face` family whose font fails to load is laid out with the fallback font
/// but not painted (reference: fonts-webfont).
#[test]
fn failed_web_font_text_is_not_painted() {
    let r = runs(
        r#"<style>@font-face { font-family: Missing; src: url(missing.ttf); }</style>
        <p style="font-family: Missing, monospace">hidden</p><p>shown</p>"#,
    );
    assert!(r.iter().all(|r| !r.text.contains("hidden")), "{r:?}");
    assert!(r.iter().any(|r| r.text.contains("shown")), "{r:?}");
}
