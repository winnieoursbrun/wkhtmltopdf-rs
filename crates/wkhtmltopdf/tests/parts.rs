//! L2 parity on the document-parts corpus (`tests/parts`, see its README):
//! headers and footers, cover, TOC, outline, `--dump-outline`, links, forms,
//! page objects, copies and grayscale. Each case runs our binary with the
//! arguments of its `cmd` file, from a fresh copy of the case directory, and
//! compares with the outputs of the real wkhtmltopdf 0.12.6:
//!
//! - exit code, stdout (empty) and stderr after normalisation (progress
//!   bars dropped, paths of the case and of the executable replaced);
//! - `--dump-outline` XML, byte for byte;
//! - `ref.json` facts read back from our PDF with lopdf: page count, sizes,
//!   page mode, info, outline (titles, levels, pages), named destinations,
//!   links (URI or destination name and page; rectangles within a
//!   tolerance), form fields, and grey colours where the case is about them;
//! - the text of every page against `ref.txt` (word similarity ≥ 0.95).
//!
//! Dates and times printed by `[date]`, `[time]` and `[isodate]` are replaced
//! by placeholders on both sides.

mod common;
#[path = "support/paths.rs"]
mod paths;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{char_ratio, ratio, words_by_page};
use lopdf::{Document, Object, ObjectId};

const MIN_SIMILARITY: f64 = 0.95;
/// Rectangles and destination positions are layout-dependent (L3).
const RECT_TOLERANCE: f64 = 10.0;
/// The reference binary's path, as it appears in XSLT error messages.
const REF_EXE_URL: &str = "file:///wk/wkhtmltopdf-linux-amd64";

fn parts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/parts")
}

struct Run {
    rc: i32,
    stdout: Vec<u8>,
    stderr: String,
    pdf: Vec<u8>,
    outline_xml: Option<String>,
    /// `file://` URL of the case directory, with a trailing slash.
    base: String,
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with("ref.") || n == "cmd" {
            continue;
        }
        fs::copy(e.path(), to.join(&name)).unwrap();
    }
}

fn run_case(case: &str) -> Run {
    let src = parts_dir().join(case);
    let args: Vec<String> = fs::read_to_string(src.join("cmd"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect();
    let tmp = std::env::temp_dir().join(format!("wk-parts-{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_dir(&src, &tmp);
    let dir = fs::canonicalize(&tmp).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(&args)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run wkhtmltopdf");
    let exe = fs::canonicalize(env!("CARGO_BIN_EXE_wkhtmltopdf")).unwrap();
    let exe_url = url_of(&exe);
    let stderr = paths::replace_dir(
        &String::from_utf8_lossy(&o.stderr).replace(&exe_url, REF_EXE_URL),
        &dir,
        &format!("/work/{case}"),
    );
    let run = Run {
        rc: o.status.code().expect("exit code"),
        stdout: o.stdout,
        stderr,
        pdf: fs::read(dir.join("out.pdf")).unwrap_or_default(),
        outline_xml: fs::read_to_string(dir.join("out.outline.xml")).ok(),
        base: format!("{}/", url_of(&dir)),
    };
    let _ = fs::remove_dir_all(&tmp);
    run
}

/// The `file://` URL the binary uses for `p` (links, XSLT messages).
fn url_of(p: &Path) -> String {
    if cfg!(windows) {
        // `file:///C:/…`, also for a canonical `\\?\C:\…` path.
        return paths::file_url(p);
    }
    let mut s = String::from("file://");
    for c in p.display().to_string().chars() {
        match c {
            ' ' => s.push_str("%20"),
            c => s.push(c),
        }
    }
    s
}

/// Phase names and messages: progress bars and padding dropped.
fn norm_stderr(s: &str) -> Vec<String> {
    s.split(['\r', '\n'])
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .collect()
}

fn is_date(w: &str) -> bool {
    let b = w.as_bytes();
    // 14 Oct 2026 is three words; match the month token only in context.
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && w.chars().filter(char::is_ascii_digit).count() == 8
}

/// Replace `[date]`, `[time]` and `[isodate]` values by placeholders, with
/// spaces around them (pdftotext glues a time to the next text).
fn norm_dates(text: &str) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let digits = |s: &[char]| s.iter().all(char::is_ascii_digit);
    while i < chars.len() {
        let rest = &chars[i..];
        // hh:mm:ss
        if rest.len() >= 8
            && digits(&rest[0..2])
            && rest[2] == ':'
            && digits(&rest[3..5])
            && rest[5] == ':'
            && digits(&rest[6..8])
        {
            out.push_str(" <TIME> ");
            i += 8;
            continue;
        }
        // yyyy-mm-dd
        if rest.len() >= 10 {
            let w: String = rest[..10].iter().collect();
            if is_date(&w) {
                out.push_str(" <ISODATE> ");
                i += 10;
                continue;
            }
        }
        // d Mon yyyy / dd Mon yyyy
        let date_len = [2usize, 1].into_iter().find(|&dl| {
            rest.len() >= dl + 9
                && digits(&rest[..dl])
                && rest[dl] == ' '
                && MONTHS.contains(&rest[dl + 1..dl + 4].iter().collect::<String>().as_str())
                && rest[dl + 4] == ' '
                && digits(&rest[dl + 5..dl + 9])
                && (i == 0 || !chars[i - 1].is_ascii_digit())
        });
        if let Some(dl) = date_len {
            out.push_str(" <DATE> ");
            i += dl + 9;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn ref_words(case: &str) -> Vec<Vec<String>> {
    let txt = fs::read_to_string(parts_dir().join(case).join("ref.txt")).unwrap();
    let mut pages: Vec<&str> = txt.split('\u{c}').collect();
    if pages.last().is_some_and(|p| p.trim().is_empty()) {
        pages.pop();
    }
    pages.iter().map(|p| norm_dates(p).split_whitespace().map(str::to_string).collect()).collect()
}

fn our_words(pdf: &[u8]) -> Vec<Vec<String>> {
    words_by_page(pdf)
        .into_iter()
        .map(|ws| norm_dates(&ws.join(" ")).split_whitespace().map(str::to_string).collect())
        .collect()
}

// ---------------------------------------------------------------------------
// PDF structure (the facts of `pdfstruct.py`), read with lopdf.

fn num(o: &Object) -> f64 {
    match o {
        Object::Integer(i) => *i as f64,
        Object::Real(r) => f64::from(*r),
        _ => 0.0,
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => doc.get_object(*r).unwrap_or(o),
        o => o,
    }
}

fn text_of(doc: &Document, o: &Object) -> String {
    match deref(doc, o) {
        Object::String(b, _) => {
            if b.starts_with(&[0xfe, 0xff]) {
                let u: Vec<u16> =
                    b[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
                String::from_utf16_lossy(&u)
            } else {
                b.iter().map(|&c| c as char).collect()
            }
        }
        Object::Name(n) => String::from_utf8_lossy(n).into_owned(),
        _ => String::new(),
    }
}

/// A link: target (`uri:…` or `dest:…`), destination page, rectangle.
type LinkFact = (String, Option<usize>, [f64; 4]);

struct Facts {
    page_count: usize,
    sizes: Vec<(f64, f64)>,
    page_mode: Option<String>,
    info: BTreeMap<String, String>,
    /// (level, title, page)
    outline: Vec<(usize, String, Option<usize>)>,
    /// Top of each bookmark's explicit destination, in outline order.
    outline_y: Vec<Option<f64>>,
    named: BTreeMap<String, usize>,
    /// Per page: (target, page, rect)
    links: Vec<Vec<LinkFact>>,
    fields: Option<Vec<serde_like::Field>>,
    non_gray: Vec<usize>,
}

mod serde_like {
    #[derive(Debug, Clone, PartialEq)]
    pub struct Field {
        pub name: String,
        pub ty: String,
        pub value: Option<String>,
        pub flags: i64,
        pub maxlen: Option<i64>,
        pub appearance_state: Option<String>,
    }
}

fn page_index(pages: &[ObjectId], o: &Object) -> Option<usize> {
    let r = o.as_reference().ok()?;
    pages.iter().position(|p| *p == r)
}

fn facts(pdf: &[u8]) -> Facts {
    let doc = Document::load_mem(pdf).expect("parse PDF");
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let catalog = doc.catalog().expect("catalog").clone();
    let page_mode = catalog.get(b"PageMode").ok().map(|o| format!("/{}", text_of(&doc, o)));
    let mut info = BTreeMap::new();
    if let Ok(i) = doc.trailer.get(b"Info") {
        if let Ok(d) = deref(&doc, i).as_dict() {
            for k in ["Title", "Creator", "Producer"] {
                if let Ok(v) = d.get(k.as_bytes()) {
                    info.insert(format!("/{k}"), text_of(&doc, v));
                }
            }
        }
    }
    // Named destinations: catalog /Dests or the /Names /Dests tree.
    let mut named = BTreeMap::new();
    let dest_page = |o: &Object| -> Option<usize> {
        match deref(&doc, o) {
            Object::Array(a) => a.first().and_then(|p| page_index(&pages, p)),
            Object::Dictionary(d) => d.get(b"D").ok().and_then(|a| match deref(&doc, a) {
                Object::Array(a) => a.first().and_then(|p| page_index(&pages, p)),
                _ => None,
            }),
            _ => None,
        }
    };
    if let Ok(d) = catalog.get(b"Dests") {
        if let Ok(d) = deref(&doc, d).as_dict() {
            for (k, v) in d.iter() {
                if let Some(p) = dest_page(v) {
                    named.insert(String::from_utf8_lossy(k).into_owned(), p);
                }
            }
        }
    }
    fn walk_names(doc: &Document, node: &Object, f: &mut dyn FnMut(String, &Object)) {
        let Ok(d) = deref(doc, node).as_dict() else { return };
        if let Ok(Object::Array(n)) = d.get(b"Names").map(|o| deref(doc, o)) {
            for pair in n.chunks(2) {
                if pair.len() == 2 {
                    f(text_of(doc, &pair[0]), &pair[1]);
                }
            }
        }
        if let Ok(Object::Array(kids)) = d.get(b"Kids").map(|o| deref(doc, o)) {
            for k in kids {
                walk_names(doc, k, f);
            }
        }
    }
    if let Ok(names) = catalog.get(b"Names") {
        if let Ok(nd) = deref(&doc, names).as_dict() {
            if let Ok(tree) = nd.get(b"Dests") {
                walk_names(&doc, tree, &mut |k, v| {
                    if let Some(p) = dest_page(v) {
                        named.insert(k, p);
                    }
                });
            }
        }
    }
    let resolve = |o: &Object| -> (String, Option<usize>) {
        match deref(&doc, o) {
            Object::String(..) | Object::Name(_) => {
                let n = text_of(&doc, o);
                let p = named.get(&n).copied();
                (format!("dest:{n}"), p)
            }
            other => ("dest:<explicit>".into(), dest_page(other)),
        }
    };
    // Outline.
    let mut outline = Vec::new();
    fn walk_outline(
        doc: &Document,
        first: Option<&Object>,
        level: usize,
        out: &mut Vec<(usize, String, Option<usize>)>,
        resolve: &dyn Fn(&Object) -> (String, Option<usize>),
    ) {
        let mut cur = first.cloned();
        let mut guard = 0;
        while let Some(o) = cur {
            guard += 1;
            if guard > 10_000 {
                break;
            }
            let Ok(d) = deref(doc, &o).as_dict() else { break };
            let title = d.get(b"Title").map(|t| text_of(doc, t)).unwrap_or_default();
            let page = if let Ok(dest) = d.get(b"Dest") {
                resolve(dest).1
            } else if let Ok(a) = d.get(b"A") {
                deref(doc, a).as_dict().ok().and_then(|a| a.get(b"D").ok()).and_then(|dd| resolve(dd).1)
            } else {
                None
            };
            out.push((level, title, page));
            walk_outline(doc, d.get(b"First").ok(), level + 1, out, resolve);
            cur = d.get(b"Next").ok().cloned();
        }
    }
    if let Ok(o) = catalog.get(b"Outlines") {
        if let Ok(od) = deref(&doc, o).as_dict() {
            walk_outline(&doc, od.get(b"First").ok(), 1, &mut outline, &resolve);
        }
    }
    fn walk_y(doc: &Document, first: Option<&Object>, out: &mut Vec<Option<f64>>) {
        let mut cur = first.cloned();
        while let Some(o) = cur {
            let Ok(d) = deref(doc, &o).as_dict() else { break };
            let y = match d.get(b"Dest").map(|o| deref(doc, o)) {
                Ok(Object::Array(a)) if a.len() > 3 => Some(num(&a[3])),
                _ => None,
            };
            out.push(y);
            walk_y(doc, d.get(b"First").ok(), out);
            cur = d.get(b"Next").ok().cloned();
            if out.len() > 10_000 {
                break;
            }
        }
    }
    let mut outline_y = Vec::new();
    if let Ok(o) = catalog.get(b"Outlines") {
        if let Ok(od) = deref(&doc, o).as_dict() {
            walk_y(&doc, od.get(b"First").ok(), &mut outline_y);
        }
    }
    // Pages: sizes, links, colours.
    let mut sizes = Vec::new();
    let mut links = Vec::new();
    let mut non_gray = Vec::new();
    for &pid in &pages {
        let pd = doc.get_object(pid).unwrap().as_dict().unwrap();
        let mut mb = None;
        let mut cur = Some(pd);
        while let Some(d) = cur {
            if let Ok(a) = d.get(b"MediaBox").map(|o| deref(&doc, o)).and_then(|o| o.as_array()) {
                mb = Some(a.iter().map(num).collect::<Vec<_>>());
                break;
            }
            cur = d.get(b"Parent").ok().and_then(|p| deref(&doc, p).as_dict().ok());
        }
        let mb = mb.unwrap_or(vec![0.0, 0.0, 0.0, 0.0]);
        sizes.push((mb[2] - mb[0], mb[3] - mb[1]));
        let mut pl = Vec::new();
        if let Ok(Object::Array(annots)) = pd.get(b"Annots").map(|o| deref(&doc, o)) {
            for a in annots {
                let Ok(a) = deref(&doc, a).as_dict() else { continue };
                if a.get(b"Subtype").and_then(|s| s.as_name()).ok() != Some(b"Link") {
                    continue;
                }
                let r: Vec<f64> = a
                    .get(b"Rect")
                    .ok()
                    .and_then(|r| deref(&doc, r).as_array().ok())
                    .map(|r| r.iter().map(num).collect())
                    .unwrap_or_default();
                let rect = [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])];
                if let Ok(act) = a.get(b"A").map(|o| deref(&doc, o)).and_then(|o| o.as_dict()) {
                    match act.get(b"S").and_then(|s| s.as_name()).ok() {
                        Some(b"URI") => {
                            pl.push((format!("uri:{}", text_of(&doc, act.get(b"URI").unwrap())), None, rect));
                        }
                        Some(b"GoTo") => {
                            let (t, p) = resolve(act.get(b"D").unwrap());
                            pl.push((t, p, rect));
                        }
                        _ => pl.push(("other".into(), None, rect)),
                    }
                } else if let Ok(d) = a.get(b"Dest") {
                    let (t, p) = resolve(d);
                    pl.push((t, p, rect));
                }
            }
        }
        links.push(pl);
        non_gray.push(non_gray_rgb(&doc, pid));
    }
    // Form fields.
    let fields = catalog.get(b"AcroForm").ok().and_then(|af| deref(&doc, af).as_dict().ok()).map(|af| {
        let mut out = Vec::new();
        if let Ok(Object::Array(fs)) = af.get(b"Fields").map(|o| deref(&doc, o)) {
            for f in fs {
                let Ok(f) = deref(&doc, f).as_dict() else { continue };
                out.push(serde_like::Field {
                    name: f.get(b"T").map(|t| text_of(&doc, t)).unwrap_or_default(),
                    ty: f.get(b"FT").map(|t| format!("/{}", text_of(&doc, t))).unwrap_or_default(),
                    value: f.get(b"V").ok().map(|v| text_of(&doc, v)),
                    flags: f.get(b"Ff").map(|v| num(deref(&doc, v)) as i64).unwrap_or(0),
                    maxlen: f.get(b"MaxLen").ok().map(|v| num(deref(&doc, v)) as i64),
                    appearance_state: f.get(b"AS").ok().map(|v| format!("/{}", text_of(&doc, v))),
                });
            }
        }
        out
    });
    Facts {
        page_count: pages.len(),
        sizes,
        page_mode,
        info,
        outline,
        outline_y,
        named,
        links,
        fields,
        non_gray,
    }
}

/// 3-component colour settings that are not grey (`r == g == b`).
fn non_gray_rgb(doc: &Document, page: ObjectId) -> usize {
    let Ok(content) = doc.get_page_content(page) else { return 0 };
    let Ok(c) = lopdf::content::Content::decode(&content) else { return 0 };
    let mut n = 0;
    for op in c.operations {
        if matches!(op.operator.as_str(), "rg" | "RG" | "scn" | "SCN" | "sc" | "SC") && op.operands.len() == 3
        {
            let v: Vec<f64> = op.operands.iter().map(|o| (num(o) * 1000.0).round()).collect();
            if !(v[0] == v[1] && v[1] == v[2]) {
                n += 1;
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------

fn json(case: &str) -> serde_json_lite::Value {
    serde_json_lite::parse(&fs::read_to_string(parts_dir().join(case).join("ref.json")).unwrap())
}

/// A small JSON reader for `ref.json` (no serde in the workspace).
mod serde_json_lite {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Value>),
        Obj(Vec<(String, Value)>),
    }

    impl Value {
        pub fn get(&self, k: &str) -> &Value {
            match self {
                Value::Obj(m) => m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v).unwrap_or(&Value::Null),
                _ => &Value::Null,
            }
        }
        pub fn arr(&self) -> &[Value] {
            match self {
                Value::Arr(a) => a,
                _ => &[],
            }
        }
        pub fn str(&self) -> Option<&str> {
            match self {
                Value::Str(s) => Some(s),
                _ => None,
            }
        }
        pub fn num(&self) -> Option<f64> {
            match self {
                Value::Num(n) => Some(*n),
                _ => None,
            }
        }
        pub fn is_null(&self) -> bool {
            matches!(self, Value::Null)
        }
    }

    pub fn parse(s: &str) -> Value {
        let b: Vec<char> = s.chars().collect();
        let mut i = 0;
        value(&b, &mut i)
    }

    fn ws(b: &[char], i: &mut usize) {
        while *i < b.len() && b[*i].is_whitespace() {
            *i += 1;
        }
    }

    fn value(b: &[char], i: &mut usize) -> Value {
        ws(b, i);
        match b[*i] {
            '{' => {
                *i += 1;
                let mut m = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == '}' {
                        *i += 1;
                        break;
                    }
                    let Value::Str(k) = value(b, i) else { panic!("key") };
                    ws(b, i);
                    *i += 1; // :
                    let v = value(b, i);
                    m.push((k, v));
                    ws(b, i);
                    if b[*i] == ',' {
                        *i += 1;
                    }
                }
                Value::Obj(m)
            }
            '[' => {
                *i += 1;
                let mut a = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == ']' {
                        *i += 1;
                        break;
                    }
                    a.push(value(b, i));
                    ws(b, i);
                    if b[*i] == ',' {
                        *i += 1;
                    }
                }
                Value::Arr(a)
            }
            '"' => {
                *i += 1;
                let mut s = String::new();
                while b[*i] != '"' {
                    if b[*i] == '\\' {
                        *i += 1;
                        match b[*i] {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'u' => {
                                let h: String = b[*i + 1..*i + 5].iter().collect();
                                s.push(char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap_or('?'));
                                *i += 4;
                            }
                            c => s.push(c),
                        }
                    } else {
                        s.push(b[*i]);
                    }
                    *i += 1;
                }
                *i += 1;
                Value::Str(s)
            }
            't' => {
                *i += 4;
                Value::Bool(true)
            }
            'f' => {
                *i += 5;
                Value::Bool(false)
            }
            'n' => {
                *i += 4;
                Value::Null
            }
            _ => {
                let st = *i;
                while *i < b.len() && (b[*i].is_ascii_digit() || "+-.eE".contains(b[*i])) {
                    *i += 1;
                }
                Value::Num(b[st..*i].iter().collect::<String>().parse().unwrap())
            }
        }
    }
}

fn strip_slash(s: &str) -> String {
    s.strip_prefix('/').unwrap_or(s).to_string()
}

fn check(case: &str) {
    let src = parts_dir().join(case);
    let run = run_case(case);
    let mut errors: Vec<String> = Vec::new();
    let want_rc: i32 = fs::read_to_string(src.join("ref.rc")).unwrap().trim().parse().unwrap();
    if run.rc != want_rc {
        errors.push(format!("exit code: ref {want_rc}, ours {}", run.rc));
    }
    if !run.stdout.is_empty() {
        errors.push(format!("stdout not empty ({} bytes)", run.stdout.len()));
    }
    let want_err = norm_stderr(&fs::read_to_string(src.join("ref.stderr")).unwrap());
    let got_err = norm_stderr(&run.stderr);
    if want_err != got_err {
        errors.push(format!("stderr:\n  ref:  {want_err:?}\n  ours: {got_err:?}"));
    }
    if let Ok(want) = fs::read_to_string(src.join("ref.outline.xml")) {
        match &run.outline_xml {
            Some(got) if *got == want => {}
            Some(got) => errors.push(format!("outline xml:\n--- ref\n{want}--- ours\n{got}")),
            None => errors.push("no outline xml written".into()),
        }
    }
    if run.pdf.is_empty() {
        errors.push("no PDF".into());
        panic!("{case}:\n{}", errors.join("\n"));
    }
    let f = facts(&run.pdf);
    let j = json(case);
    let base_sub = |s: &str| s.replace(&run.base, "{BASE}");

    let want_pages = j.get("page_count").num().unwrap() as usize;
    if f.page_count != want_pages {
        errors.push(format!("page count: ref {want_pages}, ours {}", f.page_count));
    }
    let want_sizes: Vec<(f64, f64)> = j
        .get("pages")
        .arr()
        .iter()
        .map(|p| {
            let s = p.get("size").arr();
            (s[0].num().unwrap(), s[1].num().unwrap())
        })
        .collect();
    let got_sizes: Vec<(f64, f64)> = f.sizes.iter().map(|(w, h)| (w.round(), h.round())).collect();
    if want_sizes != got_sizes {
        errors.push(format!("page sizes: ref {want_sizes:?}, ours {got_sizes:?}"));
    }
    let want_mode = j.get("page_mode").str().map(str::to_string);
    if want_mode != f.page_mode {
        errors.push(format!("page mode: ref {want_mode:?}, ours {:?}", f.page_mode));
    }
    for (k, v) in match j.get("info") {
        serde_json_lite::Value::Obj(m) => m.clone(),
        _ => vec![],
    } {
        let got = f.info.get(&k).cloned();
        if got.as_deref() != v.str() {
            errors.push(format!("info {k}: ref {:?}, ours {got:?}", v.str()));
        }
    }
    let want_outline: Vec<(usize, String, Option<usize>)> = j
        .get("outline")
        .arr()
        .iter()
        .map(|o| {
            (
                o.get("level").num().unwrap() as usize,
                o.get("title").str().unwrap().to_string(),
                o.get("page").num().map(|p| p as usize - 1),
            )
        })
        .collect();
    let want_y: Vec<Option<f64>> = j.get("outline").arr().iter().map(|o| o.get("y").num()).collect();
    for (i, (w, g)) in want_y.iter().zip(&f.outline_y).enumerate() {
        if let (Some(w), Some(g)) = (w, g) {
            if (w - g).abs() > RECT_TOLERANCE {
                errors.push(format!("outline item {} y: ref {w}, ours {g:.1}", i + 1));
            }
        }
    }
    if want_outline != f.outline {
        errors.push(format!("outline:\n  ref:  {want_outline:?}\n  ours: {:?}", f.outline));
    }
    let want_named: Vec<String> =
        j.get("named_dests").arr().iter().map(|v| strip_slash(v.str().unwrap())).collect();
    let mut got_named: Vec<String> = f.named.keys().map(|k| base_sub(&strip_slash(k))).collect();
    got_named.sort();
    if want_named != got_named {
        errors.push(format!("named dests:\n  ref:  {want_named:?}\n  ours: {got_named:?}"));
    }
    // Links, page by page, as sorted multisets of (target, page).
    for (pi, p) in j.get("pages").arr().iter().enumerate() {
        let page_h = want_sizes[pi].1;
        let mut want: Vec<(String, Option<usize>, Vec<f64>)> = p
            .get("links")
            .arr()
            .iter()
            .map(|l| {
                let rect: Vec<f64> = l.get("rect").arr().iter().map(|v| v.num().unwrap()).collect();
                if let Some(u) = l.get("uri").str() {
                    (format!("uri:{u}"), None, rect)
                } else {
                    (
                        format!("dest:{}", strip_slash(l.get("dest_name").str().unwrap_or(""))),
                        l.get("dest_page").num().map(|p| p as usize - 1),
                        rect,
                    )
                }
            })
            .collect();
        let mut got: Vec<(String, Option<usize>, Vec<f64>)> = f
            .links
            .get(pi)
            .map(|v| {
                v.iter()
                    .map(|(t, pg, r)| {
                        let t = if let Some(n) = t.strip_prefix("dest:") {
                            format!("dest:{}", base_sub(&strip_slash(n)))
                        } else {
                            base_sub(t)
                        };
                        (t, *pg, r.to_vec())
                    })
                    .collect()
            })
            .unwrap_or_default();
        want.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)).then(b.2[1].total_cmp(&a.2[1])));
        got.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)).then(b.2[1].total_cmp(&a.2[1])));
        let wk: Vec<_> = want.iter().map(|(t, p, _)| (t.clone(), *p)).collect();
        let gk: Vec<_> = got.iter().map(|(t, p, _)| (t.clone(), *p)).collect();
        if wk != gk {
            errors.push(format!("page {} links:\n  ref:  {wk:?}\n  ours: {gk:?}", pi + 1));
            continue;
        }
        for (w, g) in want.iter().zip(&got) {
            // Header/footer links of the reference lie outside the page
            // (QPainter::addLink with the header's transform): not compared.
            if w.2[3] > page_h || w.2[1] < 0.0 {
                continue;
            }
            if w.2.iter().zip(&g.2).any(|(a, b)| (a - b).abs() > RECT_TOLERANCE) {
                errors.push(format!("page {} link {} rect: ref {:?}, ours {:?}", pi + 1, w.0, w.2, g.2));
            }
        }
    }
    // Form fields.
    let want_fields = j.get("acroform_fields");
    match (&f.fields, want_fields.is_null()) {
        (None, true) => {}
        (Some(_), true) => errors.push("AcroForm present, none in ref".into()),
        (None, false) => errors.push("no AcroForm".into()),
        (Some(got), false) => {
            let want: Vec<serde_like::Field> = want_fields
                .arr()
                .iter()
                .map(|w| serde_like::Field {
                    name: w.get("name").str().unwrap().into(),
                    ty: w.get("type").str().unwrap().into(),
                    value: w.get("value").str().map(str::to_string),
                    flags: w.get("flags").num().unwrap() as i64,
                    maxlen: w.get("maxlen").num().map(|v| v as i64),
                    appearance_state: w.get("appearance_state").str().map(str::to_string),
                })
                .collect();
            if want != *got {
                errors.push(format!("form fields:\n  ref:  {want:?}\n  ours: {got:?}"));
            }
        }
    }
    // Grey colours, where the case is about them.
    for (pi, p) in j.get("pages").arr().iter().enumerate() {
        let want = p.get("colors").get("non_gray_rgb").num().unwrap_or(0.0) as usize;
        let concerned = case == "grayscale" || (case == "multi-object" && pi == 0);
        if concerned && want == 0 && f.non_gray.get(pi).copied().unwrap_or(0) != 0 {
            errors.push(format!("page {}: {} non-grey colours, ref none", pi + 1, f.non_gray[pi]));
        }
    }
    // Text.
    let rw = ref_words(case);
    let gw = our_words(&run.pdf);
    for i in 0..rw.len().max(gw.len()) {
        let (a, b) = (rw.get(i).map_or(&[][..], |v| v), gw.get(i).map_or(&[][..], |v| v));
        // Word similarity, or character similarity without whitespace (word
        // splitting depends on the extractor's gap heuristics).
        let r = ratio(a, b).max(char_ratio(&a.join(" "), &b.join(" ")));
        if r < MIN_SIMILARITY {
            errors.push(format!(
                "page {} text similarity {r:.2}\n  ref:  {}\n  ours: {}",
                i + 1,
                a.join(" "),
                b.join(" ")
            ));
        }
    }
    assert!(errors.is_empty(), "{case}: {} mismatch(es)\n{}", errors.len(), errors.join("\n"));
}

#[test]
fn date_normalisation() {
    assert_eq!(
        norm_dates("14 Oct 2026 2026-10-14 15:13:55Parts").split_whitespace().collect::<Vec<_>>(),
        vec!["<DATE>", "<ISODATE>", "<TIME>", "Parts"]
    );
    assert_eq!(norm_dates("p1 of 5"), "p1 of 5");
}

macro_rules! cases {
    ($($(#[$m:meta])* $f:ident => $name:literal;)*) => {
        $(
            #[test]
            $(#[$m])*
            fn $f() {
                check($name);
            }
        )*
    };
}

cases! {
    a5_landscape => "a5-landscape";
    copies_collate => "copies-collate";
    copies_nocollate => "copies-nocollate";
    cover_toc_pages => "cover-toc-pages";
    dump_outline_escaping => "dump-outline-escaping";
    forms => "forms";
    grayscale => "grayscale";
    hf_default_header => "hf-default-header";
    hf_html_js => "hf-html-js";
    hf_html_static => "hf-html-static";
    hf_text_style => "hf-text-style";
    hf_text_vars => "hf-text-vars";
    links => "links";
    links_disabled => "links-disabled";
    multi_object => "multi-object";
    no_outline => "no-outline";
    outline_depth => "outline-depth";
    page_offset => "page-offset";
    toc_default => "toc-default";
    toc_header_unescaped => "toc-header-unescaped";
    toc_options => "toc-options";
    toc_xsl => "toc-xsl";
}
