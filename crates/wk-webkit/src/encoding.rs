//! Document text decoding. Stands in for WebCore's `TextResourceDecoder` as
//! configured by wkhtmltopdf (`QWebSettings::DefaultTextEncoding` from
//! `--encoding`, spec §3.3).
//!
//! Precedence, like WebKit: byte order mark, then the `charset` of the HTTP
//! `Content-Type`, then a `<meta charset>` / `<meta http-equiv>` found in the
//! first 1024 bytes, then the default encoding (`--encoding`), else
//! ISO-8859-1 (decoded as windows-1252, as browsers do).

use encoding_rs::Encoding;

fn label_after(haystack: &str, key: &str) -> Option<String> {
    let i = haystack.find(key)?;
    let rest = haystack[i + key.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.trim_start_matches(['"', '\'']);
    let end =
        rest.find(|c: char| c == '"' || c == '\'' || c == ';' || c == '>' || c == '/' || c.is_whitespace());
    let label = &rest[..end.unwrap_or(rest.len())];
    if label.is_empty() { None } else { Some(label.to_string()) }
}

/// `charset` parameter of a `Content-Type` value.
pub fn charset_of_content_type(ct: &str) -> Option<&'static Encoding> {
    let lower = ct.to_ascii_lowercase();
    label_after(&lower, "charset").and_then(|l| Encoding::for_label(l.as_bytes()))
}

/// The HTML5 meta prescan, simplified: the first `charset=` inside a
/// `<meta` tag of the first 1024 bytes.
pub fn prescan_meta(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(1024)];
    let text: String =
        head.iter().map(|&b| if b.is_ascii() { b.to_ascii_lowercase() as char } else { ' ' }).collect();
    let mut from = 0;
    while let Some(i) = text[from..].find("<meta") {
        let start = from + i;
        let end = text[start..].find('>').map(|e| start + e).unwrap_or(text.len());
        let tag = &text[start..end];
        if let Some(l) = label_after(tag, "charset") {
            if let Some(e) = Encoding::for_label(l.as_bytes()) {
                // A UTF-16 label in a meta tag means UTF-8 (the bytes are ASCII-compatible).
                if e == encoding_rs::UTF_16LE || e == encoding_rs::UTF_16BE {
                    return Some(encoding_rs::UTF_8);
                }
                return Some(e);
            }
        }
        from = end;
    }
    None
}

/// Decode an HTML document.
pub fn decode_html(bytes: &[u8], content_type: Option<&str>, default_encoding: &str) -> String {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[bom_len..]).0.into_owned();
    }
    let enc = content_type
        .and_then(charset_of_content_type)
        .or_else(|| prescan_meta(bytes))
        .or_else(|| {
            if default_encoding.is_empty() {
                None
            } else {
                Encoding::for_label(default_encoding.trim().as_bytes())
            }
        })
        .unwrap_or(encoding_rs::WINDOWS_1252);
    enc.decode_without_bom_handling(bytes).0.into_owned()
}

/// Decode a stylesheet (`@charset`, BOM, else the document's default).
pub fn decode_css(bytes: &[u8], default_encoding: &str) -> String {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        return enc.decode_without_bom_handling(&bytes[bom_len..]).0.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(b"@charset \"") {
        if let Some(end) = rest.iter().position(|&b| b == b'"') {
            if let Some(e) = Encoding::for_label(&rest[..end]) {
                return e.decode_without_bom_handling(bytes).0.into_owned();
            }
        }
    }
    let enc = Encoding::for_label(default_encoding.trim().as_bytes()).unwrap_or(encoding_rs::UTF_8);
    enc.decode_without_bom_handling(bytes).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence() {
        let latin1 = b"<html><body>caf\xe9</body></html>";
        assert!(decode_html(latin1, None, "").contains("café"));
        assert!(decode_html("café".as_bytes(), None, "UTF-8").contains("café"));
        let meta = b"<meta charset=\"iso-8859-15\"><p>\xa4</p>";
        assert!(decode_html(meta, None, "UTF-8").contains('€'));
        assert!(decode_html("é".as_bytes(), Some("text/html; charset=utf-8"), "latin1").contains('é'));
        let bom = b"\xef\xbb\xbf\xc3\xa9";
        assert_eq!(decode_html(bom, None, "latin1"), "é");
    }
}
