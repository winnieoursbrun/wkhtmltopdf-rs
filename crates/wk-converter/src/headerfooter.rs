//! Header and footer variables and text. Ports the header/footer parts of
//! upstream `src/lib/pdfconverter.cc`: `fillParms`, `hfreplace`,
//! `loadHeaderFooter` (the query string of `--header-html` pages) and the
//! text drawing geometry of `endPage` (spec §3.4).

use url::Url;

/// The variables of one page (`QHash<QString, QString> parms`), in the
/// iteration order observed with the 0.12.6 binary (spec §3.4: `foo=bar`
/// first, then `page`, `section`, `sitepage`, …): `--replace` pairs first,
/// then the built-ins in [`BUILTIN_ORDER`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parms(pub Vec<(String, String)>);

/// Built-in keys in the hash order of the verified query string.
pub const BUILTIN_ORDER: [&str; 15] = [
    "page",
    "section",
    "sitepage",
    "title",
    "subsection",
    "frompage",
    "subsubsection",
    "isodate",
    "topage",
    "doctitle",
    "sitepages",
    "webpage",
    "time",
    "date",
    "",
];

impl Parms {
    /// `parms[key] = value`.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Order the entries: replacements (non built-in keys) first, in
    /// insertion order, then the built-ins.
    pub fn ordered(&self) -> Vec<(String, String)> {
        let builtin = |k: &str| BUILTIN_ORDER.iter().position(|b| *b == k && !b.is_empty());
        let mut v: Vec<(String, String)> =
            self.0.iter().filter(|(k, _)| builtin(k).is_none()).cloned().collect();
        let mut b: Vec<(usize, String, String)> =
            self.0.iter().filter_map(|(k, val)| builtin(k).map(|i| (i, k.clone(), val.clone()))).collect();
        b.sort_by_key(|(i, _, _)| *i);
        v.extend(b.into_iter().map(|(_, k, val)| (k, val)));
        v
    }
}

/// Find `needle` in `hay` from byte `from`, ignoring case
/// (`Qt::CaseInsensitive`, simple case folding per character).
fn find_ci(hay: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    let n: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if n.is_empty() {
        return None;
    }
    let mut starts = hay[from..].char_indices().map(|(i, _)| i + from);
    for s in starts.by_ref() {
        let mut it = hay[s..].char_indices();
        let mut k = 0;
        let mut end = s;
        while k < n.len() {
            let Some((i, c)) = it.next() else { break };
            let lc: Vec<char> = c.to_lowercase().collect();
            if n[k..].starts_with(&lc) {
                k += lc.len();
                end = s + i + c.len_utf8();
            } else {
                break;
            }
        }
        if k == n.len() {
            return Some((s, end));
        }
    }
    None
}

/// `hfreplace`: replace every `[key]` (case-insensitive) by its value, key
/// by key in hash order.
pub fn hfreplace(q: &str, parms: &Parms) -> String {
    let mut r = q.to_string();
    for (k, v) in parms.ordered() {
        let needle = format!("[{k}]");
        let mut from = 0;
        while let Some((a, b)) = find_ci(&r, &needle, from) {
            r.replace_range(a..b, &v);
            from = a + v.len();
        }
    }
    r
}

/// `QUrl::addQueryItem` encoding (Qt 4): `&`, `=`, `+`, `#`, `%`, spaces
/// and non-ASCII bytes are percent-encoded; `:` and `/` are kept.
fn query_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'&' | b'=' | b'+' | b'#' | b'%' | b' ' | b'"' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{'
            | b'|' | b'}' => out.push_str(&format!("%{b:02X}")),
            0..=0x20 | 0x7f..=0xff => out.push_str(&format!("%{b:02X}")),
            b => out.push(b as char),
        }
    }
    out
}

/// `loadHeaderFooter`: the header/footer URL with every variable appended
/// as a query item.
pub fn header_footer_url(base: &Url, parms: &Parms) -> Url {
    let mut s = base.to_string();
    let mut frag = String::new();
    if let Some(i) = s.find('#') {
        frag = s[i..].to_string();
        s.truncate(i);
    }
    for (k, v) in parms.ordered() {
        s.push(if s.contains('?') { '&' } else { '?' });
        s.push_str(&query_encode(&k));
        s.push('=');
        s.push_str(&query_encode(&v));
    }
    s.push_str(&frag);
    Url::parse(&s).unwrap_or_else(|_| base.clone())
}

/// Civil date from days since the Unix epoch.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `[time]`, `[date]` and `[isodate]` of the C locale
/// (`Qt::SystemLocaleShortDate`), for `secs` since the epoch in UTC.
pub fn time_and_dates(secs: u64) -> (String, String, String) {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil(days);
    let time = format!("{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60);
    let date = format!("{d} {} {y}", MONTHS[(m - 1) as usize]);
    let iso = format!("{y:04}-{m:02}-{d:02}");
    (time, date, iso)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_is_case_insensitive_and_unknown_stays() {
        let mut p = Parms::default();
        p.set("bar", "BAR");
        p.set("page", "3");
        assert_eq!(hfreplace("[PAGE] [Page] [foo] [bar]", &p), "3 3 [foo] BAR");
    }

    #[test]
    fn query_string_like_qt4() {
        let mut p = Parms::default();
        p.set("foo", "bar");
        p.set("date", "14 Oct 2026");
        p.set("time", "13:59:59");
        p.set("page", "11");
        p.set("section", "A & B=c+d#e");
        let u = header_footer_url(&Url::parse("file:///w/hdr.html").unwrap(), &p);
        assert_eq!(
            u.as_str(),
            "file:///w/hdr.html?foo=bar&page=11&section=A%20%26%20B%3Dc%2Bd%23e&time=13:59:59&date=14%20Oct%202026"
        );
    }

    #[test]
    fn c_locale_dates() {
        assert_eq!(
            time_and_dates(1_791_990_000),
            ("15:00:00".to_string(), "14 Oct 2026".to_string(), "2026-10-14".to_string())
        );
    }
}
