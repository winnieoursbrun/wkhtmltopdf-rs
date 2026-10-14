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

/// Append the update to `pdf`. `fields[i]` are the fields of page `i`;
/// rectangles are converted from top-left points with the paper height.
pub fn update(
    pdf: Vec<u8>,
    page_h: f32,
    outline_mode: bool,
    fields: &[&[FormField]],
) -> Result<Vec<u8>, String> {
    let prev = lopdf::Document::load_mem(&pdf).map_err(err)?;
    let mut inc = IncrementalDocument::create_from(pdf, prev);
    let root_id =
        inc.get_prev_documents().trailer.get(b"Root").and_then(Object::as_reference).map_err(err)?;
    let page_ids: Vec<ObjectId> = inc.get_prev_documents().get_pages().values().copied().collect();
    inc.opt_clone_object_to_new_document(root_id).map_err(err)?;
    let any_field = fields.iter().any(|f| !f.is_empty());
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
