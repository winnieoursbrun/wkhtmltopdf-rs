//! PDF helpers shared by the integration tests: page sizes, and the words
//! of each page in reading order (`pdftotext`-like) with a word-level
//! similarity ratio.

#![allow(dead_code)]

/// Page sizes (MediaBox width × height, rounded) in page order.
pub fn page_sizes(pdf: &[u8]) -> Vec<(i64, i64)> {
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

pub fn fold(s: &str) -> String {
    s.replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl")
        .replace(['\u{fb05}', '\u{fb06}'], "st")
        .replace('\u{a0}', " ")
}

/// One extracted character: position (top-left origin), advance, size.
pub struct Ch {
    x: f64,
    y: f64,
    w: f64,
    size: f64,
    s: String,
}

/// Collects positioned characters per page (`pdf_extract::OutputDev`).
#[derive(Default)]
pub struct Collect {
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
pub fn words_by_page(pdf: &[u8]) -> Vec<Vec<String>> {
    let mut doc = lopdf::Document::load_mem(pdf).expect("parse PDF");
    normalize_cid_widths(&mut doc);
    flatten_widgets(&mut doc);
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
pub fn normalize_cid_widths(doc: &mut lopdf::Document) {
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
pub fn ratio(a: &[String], b: &[String]) -> f64 {
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

/// `2·LCS / (|a| + |b|)` over the characters of two texts, whitespace
/// ignored: insensitive to how an extractor splits words.
pub fn char_ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().filter(|c| !c.is_whitespace()).collect();
    let b: Vec<char> = b.chars().filter(|c| !c.is_whitespace()).collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut prev = vec![0u32; b.len() + 1];
    let mut cur = vec![0u32; b.len() + 1];
    for x in &a {
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y { prev[j] + 1 } else { cur[j].max(prev[j + 1]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    2.0 * f64::from(prev[b.len()]) / (a.len() + b.len()) as f64
}

/// Draw the form fields into their page content, like `pdftotext` (poppler)
/// does when it renders annotations: wkhtmltopdf writes widgets without
/// appearance streams (`/NeedAppearances true`), and poppler synthesises
/// them from `/DA` and `/V` (Helvetica 12; `*` for passwords) and, for a
/// checked checkbox, a ZapfDingbats check mark. The text is put just below
/// the field so it does not interleave with the page's own (white) text.
pub fn flatten_widgets(doc: &mut lopdf::Document) {
    use lopdf::{Dictionary, Object, Stream, dictionary};
    let num = |o: &Object| match o {
        Object::Integer(i) => *i as f64,
        Object::Real(r) => f64::from(*r),
        _ => 0.0,
    };
    let deref = |doc: &lopdf::Document, o: &Object| -> Object {
        match o {
            Object::Reference(r) => doc.get_object(*r).cloned().unwrap_or(Object::Null),
            o => o.clone(),
        }
    };
    let text = |o: &Object| -> String {
        match o {
            Object::String(b, _) if b.starts_with(&[0xfe, 0xff]) => {
                let u: Vec<u16> =
                    b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
                String::from_utf16_lossy(&u)
            }
            Object::String(b, _) => b.iter().map(|&c| c as char).collect(),
            _ => String::new(),
        }
    };
    let pages: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in pages {
        let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict()).cloned() else { continue };
        let annots = match page.get(b"Annots").map(|o| deref(doc, o)) {
            Ok(Object::Array(a)) => a,
            _ => continue,
        };
        let mut ops: Vec<u8> = Vec::new();
        for a in annots {
            let Object::Dictionary(d) = deref(doc, &a) else { continue };
            if d.get(b"Subtype").and_then(|s| s.as_name()).ok() != Some(b"Widget") {
                continue;
            }
            let Ok(rect) = d.get(b"Rect").and_then(|o| o.as_array()) else { continue };
            let rect: Vec<f64> = rect.iter().map(num).collect();
            let ff = d.get(b"Ff").map(num).unwrap_or(0.0) as i64;
            let (x, y) = (rect[0].min(rect[2]) + 2.0, rect[1].min(rect[3]) - 6.0);
            match d.get(b"FT").and_then(|o| o.as_name()).ok() {
                Some(b"Tx") => {
                    let Ok(v) = d.get(b"V").map(|o| deref(doc, o)) else { continue };
                    let mut v = text(&v);
                    if ff & (1 << 13) != 0 {
                        v = "*".repeat(v.chars().count());
                    }
                    let mut bytes: Vec<u8> = Vec::new();
                    for c in v.chars() {
                        let b = if (c as u32) < 256 { c as u8 } else { b'?' };
                        if matches!(b, b'(' | b')' | b'\\') {
                            bytes.push(b'\\');
                        }
                        bytes.push(b);
                    }
                    ops.extend_from_slice(format!("BT /WkHelv 12 Tf 1 0 0 1 {x} {y} Tm (").as_bytes());
                    ops.extend_from_slice(&bytes);
                    ops.extend_from_slice(b") Tj ET\n");
                }
                Some(b"Btn") if d.get(b"AS").and_then(|o| o.as_name()).ok() == Some(b"Yes") => {
                    ops.extend_from_slice(
                        format!("BT /WkZaDb 10 Tf 1 0 0 1 {x} {y} Tm (4) Tj ET\n").as_bytes(),
                    );
                }
                _ => {}
            }
        }
        if ops.is_empty() {
            continue;
        }
        let content = doc.add_object(Stream::new(dictionary! {}, ops));
        let mut res = match page.get(b"Resources").map(|o| deref(doc, o)) {
            Ok(Object::Dictionary(r)) => r,
            _ => Dictionary::new(),
        };
        let mut fonts = match res.get(b"Font").map(|o| deref(doc, o)) {
            Ok(Object::Dictionary(f)) => f,
            _ => Dictionary::new(),
        };
        fonts.set(
            "WkHelv",
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" },
        );
        fonts.set(
            "WkZaDb",
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" },
        );
        res.set("Font", Object::Dictionary(fonts));
        let mut contents = match page.get(b"Contents") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(o) => vec![o.clone()],
            Err(_) => vec![],
        };
        contents.push(Object::Reference(content));
        if let Ok(pd) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            pd.set("Contents", Object::Array(contents));
            pd.set("Resources", Object::Dictionary(res));
        }
    }
}
