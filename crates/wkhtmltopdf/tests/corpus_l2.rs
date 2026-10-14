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

const MIN_SIMILARITY: f64 = 0.95;

/// Page sizes (MediaBox width × height, rounded) in page order.
fn page_sizes(pdf: &[u8]) -> Vec<(i64, i64)> {
    let doc = lopdf::Document::load_mem(pdf).expect("parse PDF");
    let num = |o: &lopdf::Object| match o {
        lopdf::Object::Integer(i) => *i as f64,
        lopdf::Object::Real(r) => f64::from(*r),
        _ => 0.0,
    };
    doc.get_pages()
        .values()
        .map(|&id| {
            let mut cur = doc.get_object(id).ok().and_then(|o| o.as_dict().ok());
            while let Some(d) = cur {
                if let Ok(mb) = d.get(b"MediaBox").and_then(|o| o.as_array()) {
                    let v: Vec<f64> = mb.iter().map(num).collect();
                    return ((v[2] - v[0]).round() as i64, (v[3] - v[1]).round() as i64);
                }
                cur = d
                    .get(b"Parent")
                    .ok()
                    .and_then(|p| p.as_reference().ok())
                    .and_then(|r| doc.get_object(r).ok())
                    .and_then(|o| o.as_dict().ok());
            }
            (0, 0)
        })
        .collect()
}

fn fold(s: &str) -> String {
    s.replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl")
        .replace(['\u{fb05}', '\u{fb06}'], "st")
        .replace('\u{a0}', " ")
}

/// One extracted character: position (top-left origin), advance, size.
struct Ch {
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    s: String,
}

/// Collects positioned characters per page (`pdf_extract::OutputDev`).
#[derive(Default)]
struct Collect {
    page_h: f64,
    pages: Vec<Vec<Ch>>,
}

impl pdf_extract::OutputDev for Collect {
    fn begin_page(
        &mut self,
        _: u32,
        mb: &pdf_extract::MediaBox,
        _: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.page_h = mb.ury - mb.lly;
        self.pages.push(Vec::new());
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _: f64,
        font_size: f64,
        c: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        let sx = (trm.m11 * trm.m11 + trm.m12 * trm.m12).sqrt();
        let sy = (trm.m21 * trm.m21 + trm.m22 * trm.m22).sqrt();
        let size = font_size * (sx * sy).sqrt();
        if let Some(p) = self.pages.last_mut() {
            p.push(Ch { x: trm.m31, y: self.page_h - trm.m32, w: width * size, size, s: c.to_string() });
        }
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
}

/// Words of each page in reading order, like `pdftotext` (the spike's
/// metric): characters grouped into lines by baseline, lines top to bottom,
/// characters left to right, a word break where the gap exceeds 0.1 em.
/// This makes the comparison independent of the painting order (QtWebKit
/// paints floats before in-flow text and repeated table headers last).
fn words_by_page(pdf: &[u8]) -> Vec<Vec<String>> {
    let mut doc = lopdf::Document::load_mem(pdf).expect("parse PDF");
    normalize_cid_widths(&mut doc);
    let mut c = Collect::default();
    pdf_extract::output_doc(&doc, &mut c).expect("extract text");
    c.pages
        .into_iter()
        .map(|mut chars| {
            chars.sort_by(|a, b| a.y.total_cmp(&b.y));
            let mut lines: Vec<Vec<Ch>> = Vec::new();
            for ch in chars {
                match lines.last_mut() {
                    Some(l) if (ch.y - l[0].y).abs() < 0.5 * ch.size.max(l[0].size) => l.push(ch),
                    _ => lines.push(vec![ch]),
                }
            }
            let mut words = Vec::new();
            for mut l in lines {
                l.sort_by(|a, b| a.x.total_cmp(&b.x));
                let mut cur = String::new();
                let mut end = f64::NEG_INFINITY;
                for ch in l {
                    if ch.x > end + 0.1 * ch.size && !cur.is_empty() {
                        words.push(std::mem::take(&mut cur));
                    }
                    for c in fold(&ch.s).chars() {
                        if c.is_whitespace() {
                            if !cur.is_empty() {
                                words.push(std::mem::take(&mut cur));
                            }
                        } else {
                            cur.push(c);
                        }
                    }
                    end = ch.x + ch.w;
                }
                if !cur.is_empty() {
                    words.push(cur);
                }
            }
            words
        })
        .collect()
}

/// pdf-extract 0.12 misreads the `c_first c_last w` form of a CIDFont `/W`
/// array (it only handles `c [w1 w2 …]`), which leaves every width at `/DW`
/// and scrambles character positions. Rewrite the range form, which krilla
/// writes, into the list form before extraction (the PDF is valid either
/// way; this works around the extractor).
fn normalize_cid_widths(doc: &mut lopdf::Document) {
    use lopdf::Object;
    for obj in doc.objects.values_mut() {
        let Ok(dict) = obj.as_dict_mut() else { continue };
        let Ok(Object::Array(w)) = dict.get(b"W").cloned() else { continue };
        let mut out = Vec::new();
        let mut i = 0;
        while i < w.len() {
            match (w.get(i), w.get(i + 1), w.get(i + 2)) {
                (Some(first), Some(Object::Array(list)), _) => {
                    out.push(first.clone());
                    out.push(Object::Array(list.clone()));
                    i += 2;
                }
                (Some(Object::Integer(a)), Some(Object::Integer(b)), Some(width)) => {
                    out.push(Object::Integer(*a));
                    out.push(Object::Array(vec![width.clone(); (*b - *a + 1).max(0) as usize]));
                    i += 3;
                }
                _ => break,
            }
        }
        dict.set("W", Object::Array(out));
    }
}

/// `2·LCS / (|a| + |b|)`; 1.0 for two empty sequences.
fn ratio(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut prev = vec![0u32; b.len() + 1];
    let mut cur = vec![0u32; b.len() + 1];
    for x in a {
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y { prev[j] + 1 } else { cur[j].max(prev[j + 1]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    2.0 * f64::from(prev[b.len()]) / (a.len() + b.len()) as f64
}

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
#[ignore = "WebKit-2012 profile probe: ellipsis, small-caps and the ignored/prefixed properties of docs/webkit2012-profile.md are not all reproduced yet"]
fn corpus_webkit2012() {
    check("webkit2012");
}

#[test]
#[ignore = "WebKit-2012 profile probe: -webkit-box and @supports are not reproduced yet (docs/webkit2012-profile.md)"]
fn corpus_webkit2012b() {
    check("webkit2012b");
}

#[test]
fn similarity_ratio() {
    let w = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(ratio(&w("a b c"), &w("a b c")), 1.0);
    assert!((ratio(&w("a b c d"), &w("a c d")) - 6.0 / 7.0).abs() < 1e-9);
    assert_eq!(fold("\u{fb01}n"), "fin");
}
