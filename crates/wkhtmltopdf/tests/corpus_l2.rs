//! L2 parity on the static corpus (`tests/corpus`, DESIGN.md §1): for each
//! page, our PDF from pdfkit's default invocation (HTML on stdin, PDF on
//! stdout, Letter, 0.75in margins, UTF-8) is compared with the reference
//! PDF made once by the real wkhtmltopdf 0.12.6 (`tests/corpus/ref/`,
//! regenerated with `tests/corpus/make-refs.sh`).
//!
//! Checks: same page count, same page size, and a word-level text
//! similarity of at least [`MIN_SIMILARITY`] for the whole document and for
//! every page (difflib-style ratio `2·M / (|a| + |b|)` with `M` the longest
//! common subsequence of words; typographic ligatures are folded first, as
//! wkhtmltopdf emits `ﬁ`/`ﬂ`). Known failures are `#[ignore]`d with their
//! reason; the thresholds are never lowered per page.
//!
//! Text is extracted with `pdf-extract` (positioned characters, ordered
//! like `pdftotext`), so the test runs without Docker or poppler. System fonts are disabled: only the embedded DejaVu set is used,
//! like the DejaVu-only reference image.

mod common;

use common::{fold, page_sizes, ratio, words_by_page};

const MIN_SIMILARITY: f64 = 0.95;

fn check(name: &str) {
    let html = std::fs::read(common::corpus().join(format!("{name}.html"))).unwrap();
    let reference = std::fs::read(common::corpus().join(format!("ref/{name}.pdf"))).unwrap();
    let got = common::ours(&html, &common::page_args(name));
    let (rs, gs) = (page_sizes(&reference), page_sizes(&got));
    let (rw, gw) = (words_by_page(&reference), words_by_page(&got));
    let doc = ratio(&rw.concat(), &gw.concat());
    let per: Vec<f64> = (0..rw.len().max(gw.len()))
        .map(|i| ratio(rw.get(i).map_or(&[][..], |v| v), gw.get(i).map_or(&[][..], |v| v)))
        .collect();
    let summary = format!(
        "{name}: pages ref {} ours {}; sizes ref {:?} ours {:?}; doc similarity {doc:.3}; per page {:?}",
        rs.len(),
        gs.len(),
        rs.first(),
        gs.first(),
        per.iter().map(|p| format!("{p:.2}")).collect::<Vec<_>>()
    );
    eprintln!("{summary}");
    if std::env::var_os("L2_DEBUG").is_some() {
        for (i, (r, g)) in rw.iter().zip(&gw).enumerate() {
            eprintln!("--- page {} ref: {}\n--- page {} ours: {}", i + 1, r.join(" "), i + 1, g.join(" "));
        }
    }
    assert_eq!(rs.len(), gs.len(), "page count\n{summary}");
    assert_eq!(rs, gs, "page sizes\n{summary}");
    assert!(doc >= MIN_SIMILARITY, "document text similarity\n{summary}");
    assert!(per.iter().all(|&p| p >= MIN_SIMILARITY), "per-page text similarity\n{summary}");
}

#[test]
fn corpus_a() {
    check("a");
}

#[test]
fn corpus_fonts() {
    check("fonts");
}

#[test]
fn corpus_invoice() {
    check("invoice");
}

#[test]
fn corpus_layout() {
    check("layout");
}

#[test]
fn corpus_report() {
    check("report");
}

#[test]
fn corpus_statement() {
    check("statement");
}

#[test]
fn corpus_large() {
    check("large");
}

#[test]
fn corpus_medium() {
    check("medium");
}

#[test]
#[ignore = "WebKit-2012 profile probe: `text-overflow: ellipsis` and `font-variant: small-caps` text are not reproduced (docs/webkit2012-profile.md)"]
fn corpus_webkit2012() {
    check("webkit2012");
}

#[test]
fn corpus_webkit2012b() {
    check("webkit2012b");
}

/// `--image-dpi 100` (tests/corpus/imagedpi.args): the PNG shown 84pt wide is
/// scaled down to 116x87 pixels like the reference, the JPEGs are embedded
/// unchanged (400x300).
#[test]
fn image_dpi_scales_down_non_jpeg_images() {
    let html = std::fs::read(common::corpus().join("imagedpi.html")).unwrap();
    let dims = |pdf: &[u8]| {
        let doc = lopdf::Document::load_mem(pdf).unwrap();
        let mut v: Vec<(i64, i64)> = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| s.dict.get(b"Subtype").and_then(|t| t.as_name()).ok() == Some(b"Image"))
            .map(|s| {
                let n = |k: &[u8]| s.dict.get(k).and_then(|o| o.as_i64()).unwrap_or(0);
                (n(b"Width"), n(b"Height"))
            })
            .collect();
        v.sort();
        v
    };
    let reference = std::fs::read(common::corpus().join("ref/imagedpi.pdf")).unwrap();
    let got = common::ours(&html, &common::page_args("imagedpi"));
    assert_eq!(dims(&got), dims(&reference));
    assert!(dims(&got).contains(&(116, 87)));
}

#[test]
fn similarity_ratio() {
    let w = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(ratio(&w("a b c"), &w("a b c")), 1.0);
    assert!((ratio(&w("a b c d"), &w("a c d")) - 6.0 / 7.0).abs() < 1e-9);
    assert_eq!(fold("\u{fb01}n"), "fin");
}
