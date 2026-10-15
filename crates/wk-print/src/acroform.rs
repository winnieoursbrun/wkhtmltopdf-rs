//! Catalog additions that krilla does not write, appended to its output as
//! an incremental update (lopdf): `/PageMode /UseOutlines` and the AcroForm
//! of `--enable-forms`. Stands in for the patched `QPdfEngine`
//! (`addTextField`, `addCheckBox`, `writePage`'s widget annotations) of
//! wkhtmltopdf's Qt 4.8 fork, as driven by upstream `pdfconverter.cc`
//! (`spoolPage`).
//!
//! The dictionaries are those of the 0.12.6 binary (verified with
//! `tests/parts/forms`): merged field/widget annotations whose `/Parent` is
//! the AcroForm dictionary itself, `/V` only for a non-empty value, `/Ff`
//! read-only (1), multiline (4096) and password (8192) bits, `/MaxLen` when
//! set, `/DA (/Helv 12 Tf 0 g)` for text fields, checkboxes with `/AS /Yes`
//! when checked and nothing otherwise (no `/DA`, so viewers fall back to
//! ZapfDingbats), no appearance streams (`/NeedAppearances true`), and a
//! `/DR` with Helvetica only.

use lopdf::{IncrementalDocument, Object, ObjectId, StringFormat, dictionary};

use crate::pdf::FormField;

fn err(e: lopdf::Error) -> String {
    format!("{e}")
}

/// A text string as Qt writes it: UTF-16BE with a byte order mark.
fn pdf_text(s: &str) -> Object {
    let mut b = vec![0xfe, 0xff];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(b, StringFormat::Literal)
}

fn real(v: f32) -> Object {
    Object::Real(v)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The leading decimal number of `b` and the bytes after it.
fn number(b: &[u8]) -> Option<(usize, &[u8])> {
    let n = b.iter().take_while(|c| c.is_ascii_digit()).count();
    let v = std::str::from_utf8(&b[..n]).ok()?.parse().ok()?;
    Some((v, &b[n..]))
}

/// What `lopdf::Document::load_mem(pdf)` gives for krilla's output, as far as an
/// incremental update of the catalog reads it (trailer, cross-reference start, highest
/// object number, the catalog), without parsing the other objects: the catalog, the
/// cross-reference table and the trailer are copied into a minimal PDF that lopdf parses.
/// `None` for anything but a single-subsection table starting at object 0 (then the
/// whole document is loaded).
fn catalog_only(pdf: &[u8]) -> Option<lopdf::Document> {
    let tail_at = pdf.len().saturating_sub(64);
    let sx = tail_at + rfind(&pdf[tail_at..], b"startxref")?;
    let (xref_start, _) = number(pdf.get(sx + b"startxref".len()..)?.trim_ascii_start())?;
    let xref = pdf.get(xref_start..)?.strip_prefix(b"xref\n0 ")?;
    let (count, rest) = number(xref)?;
    let entries = rest.strip_prefix(b"\n")?;
    // Entries are 20 bytes: "nnnnnnnnnn ggggg n" and a two-byte end of line.
    let trailer = entries.get(count * 20..)?;
    if !trailer.starts_with(b"trailer") {
        return None;
    }
    let trailer = &trailer[..find(trailer, b"startxref")?];
    let (root, _) = number(trailer.get(find(trailer, b"/Root ")? + b"/Root ".len()..)?)?;
    if root == 0 || root >= count {
        return None;
    }
    let entry = &entries[root * 20..root * 20 + 20];
    if &entry[10..18] != b" 00000 n" {
        return None;
    }
    let (offset, _) = number(entry)?;
    let object = pdf.get(offset..)?;
    let object = &object[..find(object, b"endobj")? + b"endobj".len()];
    if !object.starts_with(format!("{root} 0 obj").as_bytes()) {
        return None;
    }

    let mut mini = b"%PDF-1.4\n".to_vec();
    let at = mini.len();
    mini.extend_from_slice(object);
    mini.push(b'\n');
    let mini_xref = mini.len();
    mini.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
    for i in 0..count {
        let line = match i {
            0 => "0000000000 65535 f \n".to_string(),
            i if i == root => format!("{at:010} 00000 n \n"),
            _ => "0000000000 00000 f \n".to_string(),
        };
        mini.extend_from_slice(line.as_bytes());
    }
    mini.extend_from_slice(trailer);
    mini.extend_from_slice(format!("startxref\n{mini_xref}\n%%EOF").as_bytes());

    let mut doc = lopdf::Document::load_mem(&mini).ok()?;
    doc.xref_start = xref_start;
    doc.max_id = u32::try_from(count - 1).ok()?;
    doc.objects.contains_key(&(u32::try_from(root).ok()?, 0)).then_some(doc)
}

/// Append the update to `pdf`. `fields[i]` are the fields of page `i`;
/// rectangles are converted from top-left points with the paper height.
pub fn update(
    pdf: Vec<u8>,
    page_h: f32,
    outline_mode: bool,
    fields: &[&[FormField]],
) -> Result<Vec<u8>, String> {
    update_with(pdf, page_h, outline_mode, fields, true)
}

fn update_with(
    pdf: Vec<u8>,
    page_h: f32,
    outline_mode: bool,
    fields: &[&[FormField]],
    catalog_alone: bool,
) -> Result<Vec<u8>, String> {
    let any_field = fields.iter().any(|f| !f.is_empty());
    // Without fields, only the catalog is rewritten: parse it alone rather than every
    // object of the document.
    let prev = match (catalog_alone && !any_field).then(|| catalog_only(&pdf)).flatten() {
        Some(prev) => prev,
        None => lopdf::Document::load_mem(&pdf).map_err(err)?,
    };
    let mut inc = IncrementalDocument::create_from(pdf, prev);
    let root_id =
        inc.get_prev_documents().trailer.get(b"Root").and_then(Object::as_reference).map_err(err)?;
    let page_ids: Vec<ObjectId> =
        if any_field { inc.get_prev_documents().get_pages().values().copied().collect() } else { Vec::new() };
    inc.opt_clone_object_to_new_document(root_id).map_err(err)?;
    let acroform_id = any_field.then(|| inc.new_document.add_object(Object::Null));
    let mut all_fields: Vec<Object> = Vec::new();
    for (i, page_fields) in fields.iter().enumerate() {
        if page_fields.is_empty() {
            continue;
        }
        let (Some(&page_id), Some(form_id)) = (page_ids.get(i), acroform_id) else { continue };
        let mut annots = Vec::new();
        for f in page_fields.iter() {
            let rect = match f {
                FormField::Text { rect, .. } | FormField::CheckBox { rect, .. } => *rect,
            };
            let mut d = dictionary! {
                "Type" => "Annot",
                "Parent" => Object::Reference(form_id),
                "F" => 4,
                "Rect" => vec![real(rect.0), real(page_h - rect.3), real(rect.2), real(page_h - rect.1)],
            };
            match f {
                FormField::Text { value, name, multiline, password, readonly, maxlen, .. } => {
                    let mut flags = 0i64;
                    if *readonly {
                        flags |= 1;
                    }
                    if *multiline {
                        flags |= 1 << 12;
                    }
                    if *password {
                        flags |= 1 << 13;
                    }
                    d.set("BS", dictionary! { "S" => "I" });
                    d.set("FT", "Tx");
                    d.set("Subtype", "Widget");
                    d.set("P", Object::Reference(page_id));
                    if !value.is_empty() {
                        d.set("V", pdf_text(value));
                    }
                    d.set("T", pdf_text(name));
                    if *maxlen >= 0 {
                        d.set("MaxLen", i64::from(*maxlen));
                    }
                    d.set("DA", Object::string_literal("/Helv 12 Tf 0 g"));
                    d.set("Ff", flags);
                }
                FormField::CheckBox { checked, name, readonly, .. } => {
                    d.set("FT", "Btn");
                    d.set("Subtype", "Widget");
                    d.set("P", Object::Reference(page_id));
                    if *checked {
                        d.set("AS", "Yes");
                    }
                    d.set("T", pdf_text(name));
                    d.set("Ff", if *readonly { 1i64 } else { 0 });
                }
            }
            let id = inc.new_document.add_object(d);
            annots.push(Object::Reference(id));
            all_fields.push(Object::Reference(id));
        }
        inc.opt_clone_object_to_new_document(page_id).map_err(err)?;
        let existing = match inc
            .new_document
            .get_object(page_id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Annots"))
        {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(r)) => inc
                .get_prev_documents()
                .get_object(*r)
                .and_then(Object::as_array)
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut merged = existing;
        merged.extend(annots);
        let page = inc.new_document.get_object_mut(page_id).and_then(Object::as_dict_mut).map_err(err)?;
        page.set("Annots", Object::Array(merged));
    }
    if let Some(form_id) = acroform_id {
        let helv = inc.new_document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        inc.new_document.objects.insert(
            form_id,
            Object::Dictionary(dictionary! {
                "Fields" => Object::Array(all_fields),
                "DR" => dictionary! { "Font" => dictionary! { "Helv" => Object::Reference(helv) } },
                "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
                "NeedAppearances" => true,
            }),
        );
    }
    let catalog = inc.new_document.get_object_mut(root_id).and_then(Object::as_dict_mut).map_err(err)?;
    if outline_mode {
        catalog.set("PageMode", "UseOutlines");
    }
    if let Some(form_id) = acroform_id {
        catalog.set("AcroForm", Object::Reference(form_id));
    }
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| format!("{e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A krilla document with a few pages, named destinations and an outline.
    fn sample() -> Vec<u8> {
        use krilla::destination::{NamedDestination, XyzDestination};
        use krilla::geom::Point;
        use krilla::outline::{Outline, OutlineNode};
        use krilla::page::PageSettings;
        let mut doc = krilla::Document::new();
        for _ in 0..3 {
            let page = doc.start_page_with(PageSettings::from_wh(200.0, 300.0).unwrap());
            page.finish();
        }
        for (i, name) in ["a (1)", "b\\2", "c\u{e9}"].iter().enumerate() {
            let _ = doc.register_named_destination(NamedDestination::new(
                (*name).to_string(),
                XyzDestination::new(i, Point::from_xy(1.0, 2.0)),
            ));
        }
        let mut outline = Outline::new();
        outline.push_child(OutlineNode::new("Top".into(), XyzDestination::new(0, Point::from_xy(0.0, 0.0))));
        doc.set_outline(outline);
        doc.finish().unwrap()
    }

    #[test]
    fn catalog_alone_gives_the_full_load_update() {
        let pdf = sample();
        assert!(catalog_only(&pdf).is_some(), "krilla's layout is recognised");
        let fast = update_with(pdf.clone(), 300.0, true, &[], true).unwrap();
        let full = update_with(pdf, 300.0, true, &[], false).unwrap();
        assert_eq!(fast, full);
    }
}
