//! The "WebKit-2012 profile": author CSS is rewritten before stylo parses it so
//! that only what QtWebKit 2.2 (WebKit 534.34, wkhtmltopdf 0.12.6's engine)
//! understood takes effect. Stylo is a 2020s engine; left alone it would apply
//! `transform`, flexbox, `calc()`, CSS variables… that the reference binary
//! ignores, and it does not know most `-webkit-` prefixed properties the
//! reference honours.
//!
//! Each decision was checked against the real binary (`tests/corpus/
//! webkit2012*.html`) and is listed in `docs/webkit2012-profile.md`.
//!
//! The rewrite works on the source text: rule blocks and declarations are
//! found with a small scanner that respects strings, comments, escapes and
//! nesting, then each declaration is kept, dropped, renamed or has its value
//! converted. Unknown at-rules and selectors are copied unchanged.

use std::sync::Mutex;

use blitz_dom::css_rewrite::CssSource;

/// The viewport media queries see: `--viewport-size`, or 0×0 by default (the
/// 0.12.6 binary matches `max-width: 0px` and never `min-width: 1px`).
static MEDIA_VIEWPORT: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

/// The headless screen of the patched Qt (`device-width`/`device-height`).
const DEVICE: (f32, f32) = (1366.0, 768.0);

/// Set the viewport media queries are evaluated against (per page load).
pub fn set_media_viewport(width: f32, height: f32) {
    if let Ok(mut v) = MEDIA_VIEWPORT.lock() {
        *v = (width, height);
    }
}

/// Rewrite `css` (a stylesheet or a declaration list).
pub fn rewrite(css: &str, source: CssSource) -> String {
    match source {
        CssSource::Sheet => rewrite_sheet(css),
        CssSource::Declarations => rewrite_declarations(css),
    }
}

/// Install the profile into blitz-dom (idempotent).
pub fn install() {
    blitz_dom::css_rewrite::set_css_rewriter(rewrite);
}

// ---------------------------------------------------------------------------
// Scanner

/// Index after the string starting at `i` (a quote).
fn skip_string(s: &[u8], i: usize) -> usize {
    let q = s[i];
    let mut j = i + 1;
    while j < s.len() {
        match s[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            b'\n' => return j,
            _ => j += 1,
        }
    }
    s.len()
}

/// Index after the comment starting at `i` (`/*`).
fn skip_comment(s: &[u8], i: usize) -> usize {
    match s[i + 2..].windows(2).position(|w| w == b"*/") {
        Some(p) => i + 2 + p + 2,
        None => s.len(),
    }
}

/// Index of the first of `stops` at nesting depth 0 from `i`, or the end.
fn find_top(s: &[u8], mut i: usize, stops: &[u8]) -> usize {
    let mut depth = 0i32;
    while i < s.len() {
        let c = s[i];
        match c {
            b'"' | b'\'' => {
                i = skip_string(s, i);
                continue;
            }
            b'/' if s.get(i + 1) == Some(&b'*') => {
                i = skip_comment(s, i);
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            _ => {}
        }
        if depth == 0 && stops.contains(&c) {
            return i;
        }
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return i;
        }
        i += 1;
    }
    s.len()
}

/// Index of the `}` closing the block whose `{` is at `open`.
fn block_end(s: &[u8], open: usize) -> usize {
    find_top(s, open + 1, b"}")
}

// ---------------------------------------------------------------------------
// Stylesheets

fn rewrite_sheet(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    rule_list(css, &mut out);
    out
}

/// Rewrite a list of rules (a stylesheet or the inside of `@media`).
fn rule_list(css: &str, out: &mut String) {
    let s = css.as_bytes();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_whitespace() {
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b'/' && s.get(i + 1) == Some(&b'*') {
            let j = skip_comment(s, i);
            out.push_str(&css[i..j]);
            i = j;
            continue;
        }
        if c == b'<' || c == b'-' && css[i..].starts_with("-->") {
            // CDO/CDC tokens of old pages: copy.
            let j = (i + if c == b'<' { 4 } else { 3 }).min(s.len());
            out.push_str(&css[i..j]);
            i = j;
            continue;
        }
        if c == b'@' {
            let name_end = css[i + 1..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
                .map_or(s.len(), |p| i + 1 + p);
            let name = css[i + 1..name_end].to_ascii_lowercase();
            let p = find_top(s, name_end, b"{;");
            if p >= s.len() || s[p] == b';' {
                let j = (p + 1).min(s.len());
                out.push_str(&css[i..j]);
                i = j;
                continue;
            }
            let end = block_end(s, p);
            let inner = &css[p + 1..end.min(s.len())];
            let after = (end + 1).min(s.len());
            match name.as_str() {
                // Unknown to WebKit 534: the whole block is ignored.
                "supports" => {}
                "media" => {
                    out.push_str("@media ");
                    out.push_str(&media_prelude(&css[name_end..p]));
                    out.push('{');
                    rule_list(inner, out);
                    out.push('}');
                }
                "document" | "-moz-document" => {
                    out.push_str(&css[i..=p]);
                    rule_list(inner, out);
                    out.push('}');
                }
                "font-face" | "page" => {
                    out.push_str(&css[i..=p]);
                    out.push_str(&rewrite_declarations(inner));
                    out.push('}');
                }
                _ => out.push_str(&css[i..after]),
            }
            i = after;
            continue;
        }
        // A qualified rule: prelude, then a declaration block.
        let p = find_top(s, i, b"{");
        if p >= s.len() {
            out.push_str(&css[i..]);
            break;
        }
        let end = block_end(s, p);
        out.push_str(&css[i..=p]);
        out.push_str(&rewrite_declarations(&css[p + 1..end.min(s.len())]));
        out.push('}');
        i = (end + 1).min(s.len());
    }
}

/// Evaluate the size features of a media query list (`width`, `height`,
/// `device-*`, `orientation`, `aspect-ratio` and their min/max forms) against
/// QtWebKit's media viewport, leaving the rest to stylo: a true feature is
/// removed from its query, a false one turns the query into `not all`.
fn media_prelude(prelude: &str) -> String {
    let (vw, vh) = MEDIA_VIEWPORT.lock().map(|v| *v).unwrap_or((0.0, 0.0));
    let queries: Vec<String> = split_args(prelude)
        .into_iter()
        .map(|q| {
            let mut negated = false;
            let mut parts: Vec<String> = Vec::new();
            let mut verdict = true;
            let s = q.as_bytes();
            let mut i = 0;
            let mut words: Vec<String> = Vec::new();
            while i < s.len() {
                if s[i] == b'(' {
                    let end = find_top(s, i + 1, b")");
                    let group = &q[i + 1..end.min(q.len())];
                    match evaluate_feature(group, vw, vh) {
                        Some(true) => {}
                        Some(false) => verdict = false,
                        None => words.push(format!("({group})")),
                    }
                    i = end + 1;
                } else if s[i].is_ascii_whitespace() {
                    i += 1;
                } else {
                    let end =
                        q[i..].find(|c: char| c.is_ascii_whitespace() || c == '(').map_or(q.len(), |p| i + p);
                    words.push(q[i..end].to_string());
                    i = end;
                }
            }
            for w in words {
                let l = w.to_ascii_lowercase();
                if l == "not" && parts.is_empty() {
                    negated = true;
                } else if l == "only" || l == "and" {
                } else {
                    parts.push(w);
                }
            }
            if !verdict {
                return if negated { "all".to_string() } else { "not all".to_string() };
            }
            if parts.is_empty() {
                return if negated { "not all".to_string() } else { "all".to_string() };
            }
            let body = parts.join(" and ");
            if negated { format!("not {body}") } else { body }
        })
        .collect();
    queries.join(", ")
}

/// `Some(result)` for a size feature, `None` for anything else.
fn evaluate_feature(group: &str, vw: f32, vh: f32) -> Option<bool> {
    let (name, value) = group.split_once(':')?;
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim().to_ascii_lowercase();
    let (prefix, feature) = match name.strip_prefix("min-") {
        Some(f) => ("min", f.to_string()),
        None => match name.strip_prefix("max-") {
            Some(f) => ("max", f.to_string()),
            None => ("", name.clone()),
        },
    };
    let length = |v: &str| -> Option<f32> {
        let v = v.trim();
        let num = |suf: &str| v.strip_suffix(suf).and_then(|n| n.trim().parse::<f32>().ok());
        num("px")
            .or_else(|| num("em").map(|e| e * 16.0))
            .or_else(|| num("rem").map(|e| e * 16.0))
            .or_else(|| num("pt").map(|e| e * 96.0 / 72.0))
            .or_else(|| num("cm").map(|e| e * 96.0 / 2.54))
            .or_else(|| num("mm").map(|e| e * 96.0 / 25.4))
            .or_else(|| num("in").map(|e| e * 96.0))
            .or_else(|| v.parse::<f32>().ok().filter(|n| *n == 0.0))
    };
    let ratio = |v: &str| -> Option<f32> {
        let (a, b) = v.split_once('/')?;
        Some(a.trim().parse::<f32>().ok()? / b.trim().parse::<f32>().ok()?)
    };
    let cmp = |actual: f32, wanted: f32| match prefix {
        "min" => actual >= wanted,
        "max" => actual <= wanted,
        _ => actual == wanted,
    };
    Some(match feature.as_str() {
        "width" => cmp(vw, length(&value)?),
        "height" => cmp(vh, length(&value)?),
        "device-width" => cmp(DEVICE.0, length(&value)?),
        "device-height" => cmp(DEVICE.1, length(&value)?),
        "aspect-ratio" => cmp(if vh > 0.0 { vw / vh } else { 0.0 }, ratio(&value)?),
        "device-aspect-ratio" => cmp(DEVICE.0 / DEVICE.1, ratio(&value)?),
        "orientation" if prefix.is_empty() => match value.as_str() {
            "portrait" => vh >= vw,
            "landscape" => vw > vh,
            _ => return None,
        },
        _ => return None,
    })
}

/// Rewrite a declaration list (`a: b; c: d`).
fn rewrite_declarations(css: &str) -> String {
    let s = css.as_bytes();
    let mut pieces: Vec<String> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let end = find_top(s, i, b";");
        let decl = &css[i..end.min(css.len())];
        match decl.find(':') {
            Some(colon) if !decl[..colon].contains(['{', '}']) => {
                let name = decl[..colon].trim();
                let (value, important) = split_important(&decl[colon + 1..]);
                for (n, v) in declaration(name, value.trim()) {
                    pieces.push(if important { format!("{n}:{v} !important") } else { format!("{n}:{v}") });
                }
            }
            // Not a declaration (garbage or a nested rule): copy, stylo skips it.
            _ if !decl.trim().is_empty() => pieces.push(decl.to_string()),
            _ => {}
        }
        i = end + 1;
    }
    let mut out = pieces.join(";");
    if !out.is_empty() && css.trim_end().ends_with(';') {
        out.push(';');
    }
    out
}

/// `(value without !important, important)`.
fn split_important(value: &str) -> (&str, bool) {
    let t = value.trim_end();
    if let Some(bang) = t.rfind('!') {
        if t[bang + 1..].trim().eq_ignore_ascii_case("important") {
            return (&t[..bang], true);
        }
    }
    (value, false)
}

// ---------------------------------------------------------------------------
// Declarations

/// Unprefixed properties QtWebKit 2.2 did not support (or, for `opacity`,
/// whose output its PDF engine does not render): such declarations are
/// dropped. Prefix matches end with `-`.
const UNSUPPORTED: &[&str] = &[
    "transform",
    "transform-origin",
    "transform-style",
    "transform-box",
    "translate",
    "rotate",
    "scale",
    "perspective",
    "perspective-origin",
    "backface-visibility",
    "transition",
    "transition-",
    "animation",
    "animation-",
    "filter",
    "backdrop-filter",
    "mix-blend-mode",
    "isolation",
    "clip-path",
    "mask",
    "mask-",
    "opacity",
    "flex",
    "flex-",
    "order",
    "justify-content",
    "justify-items",
    "justify-self",
    "align-items",
    "align-self",
    "align-content",
    "place-",
    "grid",
    "grid-",
    "gap",
    "row-gap",
    "column-gap",
    "columns",
    "column-",
    "overflow-wrap",
    // The CSS3 fragmentation names: only `page-break-*` existed.
    "break-before",
    "break-after",
    "break-inside",
    "text-decoration-line",
    "text-decoration-color",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-underline-offset",
    "text-underline-position",
    "object-fit",
    "object-position",
    "will-change",
    "contain",
    "aspect-ratio",
    "inset",
    "hyphens",
    "tab-size",
    "font-variant-ligatures",
    "font-feature-settings",
    "font-kerning",
    "writing-mode",
    "scroll-",
    "overscroll-",
    "touch-action",
    "caret-color",
    "accent-color",
    "appearance",
    "user-select",
    "pointer-events",
    "border-block",
    "border-inline",
    "margin-block",
    "margin-inline",
    "padding-block",
    "padding-inline",
    "inline-size",
    "block-size",
    "min-inline-size",
    "max-inline-size",
    "min-block-size",
    "max-block-size",
    "inset-",
];

/// `-webkit-` properties QtWebKit 2.2 honoured, mapped to the standard name.
const PREFIXED: &[(&str, &str)] = &[
    ("transform", "transform"),
    ("transform-origin", "transform-origin"),
    ("box-shadow", "box-shadow"),
    ("border-radius", "border-radius"),
    ("border-top-left-radius", "border-top-left-radius"),
    ("border-top-right-radius", "border-top-right-radius"),
    ("border-bottom-left-radius", "border-bottom-left-radius"),
    ("border-bottom-right-radius", "border-bottom-right-radius"),
    ("box-sizing", "box-sizing"),
    ("background-size", "background-size"),
    ("background-clip", "background-clip"),
    ("background-origin", "background-origin"),
    ("border-image", "border-image"),
    ("text-size-adjust", "text-size-adjust"),
];

fn unsupported(name: &str) -> bool {
    UNSUPPORTED.iter().any(|u| {
        if let Some(p) = u.strip_suffix('-') { name.starts_with(&format!("{p}-")) } else { name == *u }
    })
}

/// The declarations replacing `name: value` (empty: dropped).
fn declaration(name: &str, value: &str) -> Vec<(String, String)> {
    let lname = name.to_ascii_lowercase();
    // Custom properties did not exist.
    if lname.starts_with("--") {
        return vec![];
    }
    let keep = |n: &str, v: String| -> Vec<(String, String)> {
        match convert_value(&v) {
            Some(v) => vec![(n.to_string(), v)],
            None => vec![],
        }
    };
    if let Some(p) = lname.strip_prefix("-webkit-") {
        if let Some((_, std)) = PREFIXED.iter().find(|(w, _)| *w == p) {
            return keep(std, value.to_string());
        }
        return legacy_flexbox(p, value);
    }
    if lname.starts_with('-') {
        // Other vendors' prefixes: unknown to WebKit.
        return vec![];
    }
    if unsupported(&lname) {
        return vec![];
    }
    if lname == "display" {
        let v = value.trim().to_ascii_lowercase();
        return match v.as_str() {
            "-webkit-box" => vec![("display".into(), "flex".into())],
            "-webkit-inline-box" => vec![("display".into(), "inline-flex".into())],
            "flex"
            | "inline-flex"
            | "grid"
            | "inline-grid"
            | "-webkit-flex"
            | "-webkit-inline-flex"
            | "flow-root"
            | "contents"
            | "ruby"
            | "math" => vec![],
            _ => keep(name, value.to_string()),
        };
    }
    if lname == "position" && value.trim().eq_ignore_ascii_case("sticky") {
        return vec![];
    }
    keep(name, value.to_string())
}

/// The 2009 flexbox (`display: -webkit-box`) properties, as flexbox.
fn legacy_flexbox(prop: &str, value: &str) -> Vec<(String, String)> {
    let v = value.trim().to_ascii_lowercase();
    let one = |n: &str, v: &str| vec![(n.to_string(), v.to_string())];
    match prop {
        "box-orient" => match v.as_str() {
            "vertical" | "block-axis" => one("flex-direction", "column"),
            _ => one("flex-direction", "row"),
        },
        "box-direction" if v == "reverse" => one("flex-direction", "row-reverse"),
        "box-pack" => match v.as_str() {
            "end" => one("justify-content", "flex-end"),
            "center" => one("justify-content", "center"),
            "justify" => one("justify-content", "space-between"),
            _ => one("justify-content", "flex-start"),
        },
        "box-align" => match v.as_str() {
            "start" => one("align-items", "flex-start"),
            "end" => one("align-items", "flex-end"),
            "center" => one("align-items", "center"),
            "baseline" => one("align-items", "baseline"),
            _ => one("align-items", "stretch"),
        },
        "box-flex" => match v.parse::<f32>() {
            Ok(f) if f >= 0.0 => vec![
                ("flex-grow".into(), format!("{f}")),
                ("flex-shrink".into(), format!("{f}")),
                ("flex-basis".into(), "auto".into()),
            ],
            _ => vec![],
        },
        "box-ordinal-group" => match v.parse::<i32>() {
            Ok(n) => one("order", &n.to_string()),
            Err(_) => vec![],
        },
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Values

/// Tokens of `value` at nesting depth 0 and inside functions are checked;
/// `None` drops the declaration (a value QtWebKit 2.2 could not parse).
fn convert_value(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    // Functions WebKit 534 did not have.
    for f in [
        "calc(",
        "-webkit-calc(",
        "var(",
        "min(",
        "max(",
        "clamp(",
        "env(",
        "attr(",
        "conic-gradient(",
        "image-set(",
        "-webkit-image-set(",
        "cross-fade(",
    ] {
        if contains_function(&lower, f) {
            return None;
        }
    }
    for f in
        ["linear-gradient(", "radial-gradient(", "repeating-linear-gradient(", "repeating-radial-gradient("]
    {
        if contains_function(&lower, f) {
            return None;
        }
    }
    if has_unsupported_unit(&lower) {
        return None;
    }
    // Prefixed gradients become standard ones.
    let mut v = value.to_string();
    for (prefixed, f) in [
        ("-webkit-repeating-linear-gradient(", legacy_linear as fn(&str) -> Option<String>),
        ("-webkit-repeating-radial-gradient(", legacy_radial),
        ("-webkit-linear-gradient(", legacy_linear),
        ("-webkit-radial-gradient(", legacy_radial),
        ("-webkit-gradient(", old_webkit_gradient),
    ] {
        v = replace_function(&v, prefixed, |args| {
            let repeating = prefixed.contains("repeating");
            f(args).map(|g| if repeating { format!("repeating-{g}") } else { g })
        })?;
    }
    Some(v)
}

/// Whether `lower` calls function `f` (`name(`) as a whole identifier.
fn contains_function(lower: &str, f: &str) -> bool {
    let mut from = 0;
    while let Some(p) = lower[from..].find(f) {
        let at = from + p;
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return true;
        }
        from = at + f.len();
    }
    false
}

/// Viewport and `ch`/`vmin`… units, which WebKit 534 did not parse.
fn has_unsupported_unit(lower: &str) -> bool {
    let s = lower.as_bytes();
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'"' | b'\'' => {
                i = skip_string(s, i);
                continue;
            }
            b'(' if lower[..i].ends_with("url") => {
                i = find_top(s, i + 1, b")") + 1;
                continue;
            }
            c if c.is_ascii_digit() || c == b'.' => {
                // A number not glued to an identifier (`h1`, `#a0b`).
                let ident_before = i > 0
                    && (s[i - 1].is_ascii_alphabetic()
                        || s[i - 1] == b'#'
                        || s[i - 1] == b'-' && i > 1 && s[i - 2].is_ascii_alphabetic());
                let mut j = i;
                while j < s.len() && (s[j].is_ascii_digit() || s[j] == b'.') {
                    j += 1;
                }
                let mut k = j;
                while k < s.len() && s[k].is_ascii_alphabetic() {
                    k += 1;
                }
                if !ident_before
                    && matches!(
                        &lower[j..k],
                        "vw" | "vh"
                            | "vmin"
                            | "vmax"
                            | "ch"
                            | "q"
                            | "dvh"
                            | "svh"
                            | "lvh"
                            | "dvw"
                            | "svw"
                            | "lvw"
                            | "cqw"
                            | "cqh"
                    )
                {
                    return true;
                }
                i = k.max(i + 1);
                continue;
            }
            b'#' => {
                // Skip hex colours whole.
                let mut j = i + 1;
                while j < s.len() && s[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                i = j;
                continue;
            }
            c if c.is_ascii_alphabetic() => {
                let mut j = i;
                while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'-') {
                    j += 1;
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Replace every call of function `f` (`name(`, case-insensitive) in `v` by
/// `conv(arguments)`; `None` if a conversion fails.
fn replace_function(v: &str, f: &str, conv: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = v;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(p) = lower.find(f).filter(|&p| {
            !lower[..p].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '-')
        }) else {
            out.push_str(rest);
            return Some(out);
        };
        let open = p + f.len() - 1;
        let close = find_top(rest.as_bytes(), open + 1, b")");
        out.push_str(&rest[..p]);
        out.push_str(&conv(&rest[open + 1..close.min(rest.len())])?);
        rest = &rest[(close + 1).min(rest.len())..];
    }
}

/// Top-level comma-separated arguments.
fn split_args(args: &str) -> Vec<&str> {
    let s = args.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i <= s.len() {
        let end = find_top(s, i, b",");
        out.push(args[i..end.min(args.len())].trim());
        i = end + 1;
    }
    out
}

fn side_opposite(k: &str) -> Option<&'static str> {
    Some(match k {
        "top" => "bottom",
        "bottom" => "top",
        "left" => "right",
        "right" => "left",
        _ => return None,
    })
}

/// `-webkit-linear-gradient(<start> | <angle>, stops…)`: the legacy syntax
/// names the side the gradient starts from, and its angles run
/// counter-clockwise from east (standard angle = 90° − legacy).
fn legacy_linear(args: &str) -> Option<String> {
    let parts = split_args(args);
    let first = parts.first()?.to_ascii_lowercase();
    let words: Vec<&str> = first.split_whitespace().collect();
    if !words.is_empty() && words.iter().all(|w| side_opposite(w).is_some() || *w == "center") {
        let to: Vec<&str> = words.iter().filter_map(|w| side_opposite(w)).collect();
        let dir = if to.is_empty() { "to bottom".to_string() } else { format!("to {}", to.join(" ")) };
        return Some(format!("linear-gradient({dir}, {})", parts[1..].join(", ")));
    }
    if let Some(deg) = angle_degrees(&first) {
        return Some(format!("linear-gradient({}deg, {})", 90.0 - deg, parts[1..].join(", ")));
    }
    Some(format!("linear-gradient({})", parts.join(", ")))
}

fn angle_degrees(s: &str) -> Option<f32> {
    let s = s.trim();
    let num = |suffix: &str| s.strip_suffix(suffix).and_then(|n| n.trim().parse::<f32>().ok());
    if let Some(v) = num("deg") {
        Some(v)
    } else if let Some(v) = num("grad") {
        Some(v * 0.9)
    } else if let Some(v) = num("rad") {
        Some(v.to_degrees())
    } else {
        num("turn").map(|v| v * 360.0)
    }
}

/// `-webkit-radial-gradient([<position>,] [<shape> || <size>,] stops…)`.
fn legacy_radial(args: &str) -> Option<String> {
    let parts = split_args(args);
    let mut idx = 0;
    let is_pos = |p: &str| {
        let l = p.to_ascii_lowercase();
        l.split_whitespace().all(|w| {
            matches!(w, "top" | "bottom" | "left" | "right" | "center")
                || w.ends_with('%')
                || w.ends_with("px")
                || w.parse::<f32>().is_ok()
        })
    };
    let is_shape = |p: &str| {
        let l = p.to_ascii_lowercase();
        l.split_whitespace().all(|w| {
            matches!(
                w,
                "circle"
                    | "ellipse"
                    | "closest-side"
                    | "closest-corner"
                    | "farthest-side"
                    | "farthest-corner"
                    | "contain"
                    | "cover"
            )
        })
    };
    let mut pos = None;
    let mut shape = None;
    if parts.first().is_some_and(|p| is_pos(p)) {
        pos = Some(parts[0]);
        idx = 1;
    }
    if parts.get(idx).is_some_and(|p| is_shape(p)) {
        shape = Some(parts[idx].replace("contain", "closest-side").replace("cover", "farthest-corner"));
        idx += 1;
    }
    let mut head = shape.unwrap_or_default();
    if let Some(p) = pos {
        if !head.is_empty() {
            head.push(' ');
        }
        head.push_str("at ");
        head.push_str(p);
    }
    let stops = parts[idx..].join(", ");
    Some(if head.is_empty() {
        format!("radial-gradient({stops})")
    } else {
        format!("radial-gradient({head}, {stops})")
    })
}

/// A `-webkit-gradient()` point (`left top`, `50% 0`, `0 100`) as fractions.
fn point(p: &str) -> (f32, f32) {
    let mut w = p.split_whitespace();
    let coord = |t: Option<&str>| -> f32 {
        match t.map(|t| t.to_ascii_lowercase()) {
            Some(t) if t == "left" || t == "top" => 0.0,
            Some(t) if t == "right" || t == "bottom" => 1.0,
            Some(t) if t == "center" => 0.5,
            Some(t) if t.ends_with('%') => t.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0,
            // Plain numbers are pixels; without the box size, keep the sign.
            Some(t) => t.parse::<f32>().map(|v| if v > 0.0 { 1.0 } else { 0.0 }).unwrap_or(0.0),
            None => 0.5,
        }
    };
    (coord(w.next()), coord(w.next()))
}

/// `-webkit-gradient(linear|radial, …, from(), to(), color-stop())`.
fn old_webkit_gradient(args: &str) -> Option<String> {
    let parts = split_args(args);
    let kind = parts.first()?.to_ascii_lowercase();
    let mut stops: Vec<(f32, String)> = Vec::new();
    let stop_parts: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| {
            let l = p.to_ascii_lowercase();
            l.starts_with("from(") || l.starts_with("to(") || l.starts_with("color-stop(")
        })
        .collect();
    for p in stop_parts {
        let l = p.to_ascii_lowercase();
        let inner = &p[p.find('(')? + 1..p.rfind(')')?];
        if l.starts_with("from(") {
            stops.push((0.0, inner.trim().to_string()));
        } else if l.starts_with("to(") {
            stops.push((1.0, inner.trim().to_string()));
        } else {
            let a = split_args(inner);
            let pos = a.first()?.trim();
            let pos = if let Some(pc) = pos.strip_suffix('%') {
                pc.parse::<f32>().ok()? / 100.0
            } else {
                pos.parse::<f32>().ok()?
            };
            stops.push((pos, a.get(1)?.trim().to_string()));
        }
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    let list = stops.iter().map(|(p, c)| format!("{c} {}%", p * 100.0)).collect::<Vec<_>>().join(", ");
    if kind == "linear" {
        let (x0, y0) = point(parts.get(1)?);
        let (x1, y1) = point(parts.get(2)?);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let mut to = Vec::new();
        if dy > 0.0 {
            to.push("bottom");
        } else if dy < 0.0 {
            to.push("top");
        }
        if dx > 0.0 {
            to.push("right");
        } else if dx < 0.0 {
            to.push("left");
        }
        let dir = if to.is_empty() { "to bottom".to_string() } else { format!("to {}", to.join(" ")) };
        Some(format!("linear-gradient({dir}, {list})"))
    } else {
        Some(format!("radial-gradient(circle, {list})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(s: &str) -> String {
        rewrite(s, CssSource::Sheet)
    }

    #[test]
    fn unprefixed_transform_is_dropped_and_prefixed_applies() {
        assert_eq!(sheet(".a{transform:rotate(3deg);color:red}"), ".a{color:red}");
        assert_eq!(sheet(".a{-webkit-transform:rotate(3deg)}"), ".a{transform:rotate(3deg)}");
    }

    #[test]
    fn only_the_page_break_names_break_pages() {
        assert_eq!(
            sheet("a{break-before:page;break-after:page;break-inside:avoid;page-break-before:always}"),
            "a{page-break-before:always}"
        );
        assert_eq!(
            sheet("a{-webkit-column-break-before:always;page-break-inside:avoid}"),
            "a{page-break-inside:avoid}"
        );
    }

    #[test]
    fn values_webkit_could_not_parse_drop_the_declaration() {
        assert_eq!(
            sheet("a{width:calc(100% - 2px);height:2vh;color:var(--x);--x:red;margin:1em}"),
            "a{margin:1em}"
        );
        assert_eq!(sheet("a{background:linear-gradient(red,blue)}"), "a{}");
        assert_eq!(sheet("h1{width:10px}"), "h1{width:10px}");
        assert_eq!(sheet("a{color:#0ch}"), "a{color:#0ch}");
    }

    #[test]
    fn supports_blocks_vanish_media_blocks_are_rewritten() {
        assert_eq!(sheet("@supports (x:y){a{color:red}}b{c:d}"), "b{c:d}");
        assert_eq!(sheet("@media print{a{transform:none;color:red}}"), "@media print{a{color:red}}");
        assert_eq!(sheet("@import url(a.css);a{b:c}"), "@import url(a.css);a{b:c}");
    }

    #[test]
    fn media_size_features_use_the_qtwebkit_viewport() {
        // The default 0x0 viewport (other tests may not change it).
        assert_eq!(sheet("@media (max-width: 0px){a{b:c}}"), "@media all{a{b:c}}");
        assert_eq!(sheet("@media screen and (min-width: 600px){a{b:c}}"), "@media not all{a{b:c}}");
        assert_eq!(sheet("@media print, (min-device-width: 1300px){a{b:c}}"), "@media print, all{a{b:c}}");
        assert_eq!(sheet("@media only screen and (color){a{b:c}}"), "@media screen and (color){a{b:c}}");
    }

    #[test]
    fn legacy_gradients_become_standard() {
        assert_eq!(
            sheet("a{background:-webkit-linear-gradient(left, red, blue)}"),
            "a{background:linear-gradient(to right, red, blue)}"
        );
        assert_eq!(
            sheet("a{background:-webkit-linear-gradient(45deg, red, blue)}"),
            "a{background:linear-gradient(45deg, red, blue)}"
        );
        assert_eq!(
            sheet("a{background:-webkit-gradient(linear, left top, left bottom, from(#c00), to(#00c))}"),
            "a{background:linear-gradient(to bottom, #c00 0%, #00c 100%)}"
        );
        assert_eq!(
            sheet("a{background:-webkit-radial-gradient(center, circle, red, blue)}"),
            "a{background:radial-gradient(circle at center, red, blue)}"
        );
    }

    #[test]
    fn legacy_flexbox_maps_to_flexbox() {
        assert_eq!(
            sheet("a{display:-webkit-box;-webkit-box-pack:justify;-webkit-box-orient:vertical}"),
            "a{display:flex;justify-content:space-between;flex-direction:column}"
        );
        assert_eq!(sheet("a{display:flex}"), "a{}");
        assert_eq!(sheet("a{-webkit-box-flex:2}"), "a{flex-grow:2;flex-shrink:2;flex-basis:auto}");
    }

    #[test]
    fn important_strings_and_comments_survive() {
        assert_eq!(sheet("a{color:red !important}"), "a{color:red !important}");
        assert_eq!(sheet("a{content:\"a;b}\"}/* x{ */b{c:d}"), "a{content:\"a;b}\"}/* x{ */b{c:d}");
        assert_eq!(rewrite("color: red; transform: none", CssSource::Declarations), "color:red");
    }
}
