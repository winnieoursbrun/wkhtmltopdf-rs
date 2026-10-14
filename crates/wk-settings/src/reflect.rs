//! Get/set settings by dotted name, as used by the libwkhtmltox C bindings
//! (`wkhtmltopdf_set_global_setting("margin.top", "2cm")`, …).
//!
//! Ports upstream `src/lib/reflect.hh`, `src/lib/reflect.cc` and the
//! `ReflectImpl<…>` specialisations of `src/lib/pdfsettings.cc`.
//!
//! Names use the upstream (camelCase) field names. Lists support `size`,
//! `length`, `count`, `[i]`, `first`, `last` on get and `clear`, `append`,
//! `prepend`, `delete`, `[i]`, `first`, `last` on set.
//!
//! Deviation: upstream returns a null `QString` for an unknown name, which the
//! C API reports as "not found"; a default-constructed (null) but valid string
//! field would also report "not found" there. Here every known name returns
//! `Some`, unknown names return `None`. Setting an out-of-range list index is
//! undefined behaviour upstream; here it fails.

use crate::loadsettings::{
    LoadErrorHandling, LoadGlobal, LoadPage, PostItem, Proxy, load_error_handling_to_str, proxy_to_str,
    str_to_load_error_handling, str_to_proxy,
};
use crate::logging::{LogLevel, log_level_to_str, str_to_log_level};
use crate::pdfsettings::{
    ColorMode, HeaderFooter, Margin, Orientation, PageSize, PdfGlobal, PdfObject, PrinterMode, Size,
    TableOfContent, UnitReal, color_mode_to_str, orientation_to_str, page_size_to_str, printer_mode_to_str,
    str_to_color_mode, str_to_orientation, str_to_page_size, str_to_printer_mode, str_to_unit_real,
    unit_real_to_str,
};
use crate::qt;
use crate::websettings::Web;

/// `settings::Reflect`.
pub trait Reflect {
    /// Value of the setting at `name`, `None` if there is no such setting.
    fn reflect_get(&self, name: &str) -> Option<String>;
    /// Set the setting at `name`; `false` if unknown or the value is invalid.
    fn reflect_set(&mut self, name: &str, value: &str) -> bool;
}

/// `ReflectSimple`: a leaf value, only reachable with an empty remaining name.
trait Simple {
    fn simple_get(&self) -> String;
    /// Parse and assign. Upstream assigns even when parsing fails.
    fn simple_set(&mut self, value: &str) -> bool;
}

macro_rules! impl_reflect_for_simple {
    ($($t:ty),*) => {$(
        impl Reflect for $t {
            fn reflect_get(&self, name: &str) -> Option<String> {
                if name.is_empty() { Some(self.simple_get()) } else { None }
            }
            fn reflect_set(&mut self, name: &str, value: &str) -> bool {
                name.is_empty() && self.simple_set(value)
            }
        }
    )*};
}

impl Simple for bool {
    fn simple_get(&self) -> String {
        if *self { "true" } else { "false" }.into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        match value {
            "true" => *self = true,
            "false" => *self = false,
            _ => return false,
        }
        true
    }
}

impl Simple for String {
    fn simple_get(&self) -> String {
        self.clone()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        *self = value.to_string();
        true
    }
}

impl Simple for i32 {
    fn simple_get(&self) -> String {
        self.to_string()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let r = qt::to_int(value);
        *self = r.unwrap_or(0);
        r.is_some()
    }
}

impl Simple for f32 {
    fn simple_get(&self) -> String {
        qt::number_g(f64::from(*self))
    }
    fn simple_set(&mut self, value: &str) -> bool {
        // Upstream uses QString::toDouble here, not toFloat.
        let r = qt::to_double(value);
        *self = r.unwrap_or(0.0) as f32;
        r.is_some()
    }
}

impl Simple for (String, String) {
    fn simple_get(&self) -> String {
        format!("{}\n{}", self.0, self.1)
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let parts: Vec<&str> = value.split('\n').collect();
        if parts.len() != 2 {
            return false;
        }
        self.0 = parts[0].to_string();
        self.1 = parts[1].to_string();
        true
    }
}

impl Simple for LogLevel {
    fn simple_get(&self) -> String {
        log_level_to_str(*self).unwrap_or("").to_string()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (l, ok) = str_to_log_level(value);
        *self = l;
        ok
    }
}

impl Simple for LoadErrorHandling {
    fn simple_get(&self) -> String {
        load_error_handling_to_str(*self).into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (l, ok) = str_to_load_error_handling(value);
        *self = l;
        ok
    }
}

impl Simple for Proxy {
    fn simple_get(&self) -> String {
        proxy_to_str(self)
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (p, ok) = str_to_proxy(value);
        *self = p;
        ok
    }
}

impl Simple for UnitReal {
    fn simple_get(&self) -> String {
        unit_real_to_str(self).unwrap_or_default()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (u, ok) = str_to_unit_real(value);
        *self = u;
        ok
    }
}

impl Simple for PageSize {
    fn simple_get(&self) -> String {
        page_size_to_str(*self).into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (p, ok) = str_to_page_size(value);
        *self = p;
        ok
    }
}

impl Simple for Orientation {
    fn simple_get(&self) -> String {
        orientation_to_str(*self).into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (o, ok) = str_to_orientation(value);
        *self = o;
        ok
    }
}

impl Simple for PrinterMode {
    fn simple_get(&self) -> String {
        printer_mode_to_str(*self).into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (m, ok) = str_to_printer_mode(value);
        *self = m;
        ok
    }
}

impl Simple for ColorMode {
    fn simple_get(&self) -> String {
        color_mode_to_str(*self).into()
    }
    fn simple_set(&mut self, value: &str) -> bool {
        let (m, ok) = str_to_color_mode(value);
        *self = m;
        ok
    }
}

impl_reflect_for_simple!(
    bool,
    String,
    i32,
    f32,
    (String, String),
    LogLevel,
    LoadErrorHandling,
    Proxy,
    UnitReal,
    PageSize,
    Orientation,
    PrinterMode,
    ColorMode
);

/// `QuietArgBackwardsCompatReflect`: the fake `quiet` global setting.
struct Quiet<'a>(&'a mut LogLevel);

impl Quiet<'_> {
    fn get(level: LogLevel) -> String {
        if level == LogLevel::None { "true" } else { "false" }.into()
    }
    fn set(&mut self, value: &str) -> bool {
        match value {
            "true" => *self.0 = LogLevel::None,
            "false" => *self.0 = LogLevel::Info,
            _ => return false,
        }
        true
    }
}

/// Split `name` at the first `.` or `[` like `ReflectClass::get/set`:
/// returns the key and the remainder (a leading `.` is consumed, a `[` kept).
fn split_head(name: &str) -> (&str, &str) {
    let i = name.find(['.', '[']).unwrap_or(name.len());
    let rest = &name[i..];
    (&name[..i], rest.strip_prefix('.').unwrap_or(rest))
}

/// Result of `ReflectImpl<QList<X>>::parse`.
struct ListRef<'a> {
    elm: i64,
    rest: &'a str,
}

fn list_parse(name: &str, len: usize) -> Option<ListRef<'_>> {
    let b = name.as_bytes();
    if b.first() == Some(&b'[') {
        let mut next = 0;
        while next < b.len() && b[next] != b']' {
            next += 1;
        }
        let inner = &name[1..next.max(1)];
        let elm = qt::to_int(inner);
        while next < b.len() && (b[next] == b']' || b[next] == b'.') {
            next += 1;
        }
        return elm.map(|e| ListRef { elm: i64::from(e), rest: &name[next..] });
    }
    let ps = name.find(['.', '[']).unwrap_or(name.len());
    let mut next = ps;
    if b.get(next) == Some(&b'.') {
        next += 1;
    }
    let head = &b[..ps];
    let mut elm = -1;
    // Upstream compares with strncmp(name, "first", ps): any prefix matches.
    if ps > 0 && len > 0 && b"first".starts_with(head) {
        elm = 0;
    }
    if ps > 0 && len > 0 && b"last".starts_with(head) {
        elm = len as i64 - 1;
    }
    Some(ListRef { elm, rest: &name[next..] })
}

impl<X: Reflect + Default> Reflect for Vec<X> {
    fn reflect_get(&self, name: &str) -> Option<String> {
        if matches!(name, "size" | "length" | "count") {
            return Some(self.len().to_string());
        }
        let r = list_parse(name, self.len())?;
        if r.elm < 0 || r.elm as usize >= self.len() {
            return None;
        }
        self[r.elm as usize].reflect_get(r.rest)
    }

    fn reflect_set(&mut self, name: &str, value: &str) -> bool {
        match name {
            "clear" => {
                self.clear();
                true
            }
            "append" => {
                self.push(X::default());
                true
            }
            "prepend" => {
                self.insert(0, X::default());
                true
            }
            "delete" => match qt::to_int(value) {
                Some(idx) if idx >= 0 && (idx as usize) < self.len() => {
                    self.remove(idx as usize);
                    true
                }
                _ => false,
            },
            _ => {
                let len = self.len();
                let Some(r) = list_parse(name, len) else { return false };
                if r.elm < 0 || r.elm as usize >= len {
                    return false;
                }
                self[r.elm as usize].reflect_set(r.rest, value)
            }
        }
    }
}

/// Implements `Reflect` for a settings struct from a list of
/// `"upstreamName" => field` pairs (a `ReflectClass`).
macro_rules! reflect_class {
    ($t:ty { $($name:literal => $field:ident),* $(,)? }) => {
        impl Reflect for $t {
            fn reflect_get(&self, name: &str) -> Option<String> {
                let (key, rest) = split_head(name);
                match key {
                    $($name => self.$field.reflect_get(rest),)*
                    _ => None,
                }
            }
            fn reflect_set(&mut self, name: &str, value: &str) -> bool {
                let (key, rest) = split_head(name);
                match key {
                    $($name => self.$field.reflect_set(rest, value),)*
                    _ => false,
                }
            }
        }
    };
}

reflect_class!(PostItem { "name" => name, "value" => value, "file" => file });
reflect_class!(LoadGlobal { "cookieJar" => cookie_jar });
reflect_class!(LoadPage {
    "username" => username,
    "password" => password,
    "clientSslKeyPath" => client_ssl_key_path,
    "clientSslKeyPassword" => client_ssl_key_password,
    "clientSslCrtPath" => client_ssl_crt_path,
    "jsdelay" => jsdelay,
    "windowStatus" => window_status,
    "zoomFactor" => zoom_factor,
    "customHeaders" => custom_headers,
    "repeatCustomHeaders" => repeat_custom_headers,
    "cookies" => cookies,
    "post" => post,
    "blockLocalFileAccess" => block_local_file_access,
    "allowed" => allowed,
    "stopSlowScripts" => stop_slow_scripts,
    "debugJavascript" => debug_javascript,
    "loadErrorHandling" => load_error_handling,
    "proxy" => proxy,
    "runScript" => run_script,
    "checkboxSvg" => checkbox_svg,
    "checkboxCheckedSvg" => checkbox_checked_svg,
    "radiobuttonSvg" => radiobutton_svg,
    "radiobuttonCheckedSvg" => radiobutton_checked_svg,
    "cacheDir" => cache_dir,
    "bypassProxyForHosts" => bypass_proxy_for_hosts,
    "proxyHostNameLookup" => proxy_host_name_lookup,
    "printMediaType" => print_media_type,
});
reflect_class!(Web {
    "background" => background,
    "loadImages" => load_images,
    "enableJavascript" => enable_javascript,
    "enableIntelligentShrinking" => enable_intelligent_shrinking,
    "minimumFontSize" => minimum_font_size,
    "defaultEncoding" => default_encoding,
    "userStyleSheet" => user_style_sheet,
    "enablePlugins" => enable_plugins,
});
reflect_class!(Margin { "top" => top, "right" => right, "bottom" => bottom, "left" => left });
reflect_class!(Size { "pageSize" => page_size, "height" => height, "width" => width });
reflect_class!(TableOfContent {
    "useDottedLines" => use_dotted_lines,
    "captionText" => caption_text,
    "forwardLinks" => forward_links,
    "backLinks" => back_links,
    "indentation" => indentation,
    "fontScale" => font_scale,
});
reflect_class!(HeaderFooter {
    "fontSize" => font_size,
    "fontName" => font_name,
    "left" => left,
    "right" => right,
    "center" => center,
    "line" => line,
    "htmlUrl" => html_url,
    "spacing" => spacing,
});
reflect_class!(PdfObject {
    "toc" => toc,
    "page" => page,
    "header" => header,
    "footer" => footer,
    "useExternalLinks" => use_external_links,
    "useLocalLinks" => use_local_links,
    "replacements" => replacements,
    "produceForms" => produce_forms,
    "load" => load,
    "web" => web,
    "includeInOutline" => include_in_outline,
    "pagesCount" => pages_count,
    "isTableOfContent" => is_table_of_content,
    "tocXsl" => toc_xsl,
});

impl Reflect for PdfGlobal {
    fn reflect_get(&self, name: &str) -> Option<String> {
        let (key, rest) = split_head(name);
        match key {
            "size" => self.size.reflect_get(rest),
            "quiet" => rest.is_empty().then(|| Quiet::get(self.log_level)),
            "logLevel" => self.log_level.reflect_get(rest),
            "useGraphics" => self.use_graphics.reflect_get(rest),
            "resolveRelativeLinks" => self.resolve_relative_links.reflect_get(rest),
            "orientation" => self.orientation.reflect_get(rest),
            "colorMode" => self.color_mode.reflect_get(rest),
            "resolution" => self.resolution.reflect_get(rest),
            "dpi" => self.dpi.reflect_get(rest),
            "pageOffset" => self.page_offset.reflect_get(rest),
            "copies" => self.copies.reflect_get(rest),
            "collate" => self.collate.reflect_get(rest),
            "outline" => self.outline.reflect_get(rest),
            "outlineDepth" => self.outline_depth.reflect_get(rest),
            "dumpOutline" => self.dump_outline.reflect_get(rest),
            "out" => self.out.reflect_get(rest),
            "documentTitle" => self.document_title.reflect_get(rest),
            "useCompression" => self.use_compression.reflect_get(rest),
            "margin" => self.margin.reflect_get(rest),
            "imageDPI" => self.image_dpi.reflect_get(rest),
            "imageQuality" => self.image_quality.reflect_get(rest),
            "load" => self.load.reflect_get(rest),
            "viewportSize" => self.viewport_size.reflect_get(rest),
            _ => None,
        }
    }

    fn reflect_set(&mut self, name: &str, value: &str) -> bool {
        let (key, rest) = split_head(name);
        match key {
            "size" => self.size.reflect_set(rest, value),
            "quiet" => rest.is_empty() && Quiet(&mut self.log_level).set(value),
            "logLevel" => self.log_level.reflect_set(rest, value),
            "useGraphics" => self.use_graphics.reflect_set(rest, value),
            "resolveRelativeLinks" => self.resolve_relative_links.reflect_set(rest, value),
            "orientation" => self.orientation.reflect_set(rest, value),
            "colorMode" => self.color_mode.reflect_set(rest, value),
            "resolution" => self.resolution.reflect_set(rest, value),
            "dpi" => self.dpi.reflect_set(rest, value),
            "pageOffset" => self.page_offset.reflect_set(rest, value),
            "copies" => self.copies.reflect_set(rest, value),
            "collate" => self.collate.reflect_set(rest, value),
            "outline" => self.outline.reflect_set(rest, value),
            "outlineDepth" => self.outline_depth.reflect_set(rest, value),
            "dumpOutline" => self.dump_outline.reflect_set(rest, value),
            "out" => self.out.reflect_set(rest, value),
            "documentTitle" => self.document_title.reflect_set(rest, value),
            "useCompression" => self.use_compression.reflect_set(rest, value),
            "margin" => self.margin.reflect_set(rest, value),
            "imageDPI" => self.image_dpi.reflect_set(rest, value),
            "imageQuality" => self.image_quality.reflect_set(rest, value),
            "load" => self.load.reflect_set(rest, value),
            "viewportSize" => self.viewport_size.reflect_set(rest, value),
            _ => false,
        }
    }
}

impl PdfGlobal {
    /// `PdfGlobal::get`.
    pub fn get(&self, name: &str) -> Option<String> {
        self.reflect_get(name)
    }
    /// `PdfGlobal::set`.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        self.reflect_set(name, value)
    }
}

impl PdfObject {
    /// `PdfObject::get`.
    pub fn get(&self, name: &str) -> Option<String> {
        self.reflect_get(name)
    }
    /// `PdfObject::set`.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        self.reflect_set(name, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdfsettings::Unit;

    #[test]
    fn global_get_set_nested() {
        let mut g = PdfGlobal::default();
        assert_eq!(g.get("margin.left").as_deref(), Some("10mm"));
        assert_eq!(g.get("margin.top").as_deref(), Some(""));
        assert!(g.set("margin.top", "2cm"));
        assert_eq!(g.margin.top, UnitReal::new(20.0, Unit::Millimeter));
        assert_eq!(g.get("size.pageSize").as_deref(), Some("A4"));
        assert!(g.set("size.pageSize", "letter"));
        assert_eq!(g.get("size.pageSize").as_deref(), Some("Letter"));
        assert_eq!(g.get("load.cookieJar").as_deref(), Some(""));
        assert_eq!(g.get("nope"), None);
        assert_eq!(g.get("dpi.x"), None);
        assert!(!g.set("dpi", "abc"));
    }

    #[test]
    fn global_quiet_backwards_compat() {
        let mut g = PdfGlobal::default();
        assert_eq!(g.get("quiet").as_deref(), Some("false"));
        assert!(g.set("quiet", "true"));
        assert_eq!(g.log_level, LogLevel::None);
        assert_eq!(g.get("logLevel").as_deref(), Some("none"));
        assert!(!g.set("quiet", "yes"));
    }

    #[test]
    fn object_lists() {
        let mut o = PdfObject::default();
        assert_eq!(o.get("load.customHeaders.size").as_deref(), Some("0"));
        assert!(o.set("load.customHeaders.append", ""));
        assert!(o.set("load.customHeaders[0]", "X-A\nb"));
        assert_eq!(o.get("load.customHeaders[0]").as_deref(), Some("X-A\nb"));
        assert_eq!(o.get("load.customHeaders.first").as_deref(), Some("X-A\nb"));
        assert!(!o.set("load.customHeaders[0]", "no-newline"));
        assert!(o.set("load.post.append", ""));
        assert!(o.set("load.post[0].name", "n"));
        assert!(o.set("load.post.last.file", "true"));
        assert_eq!(o.load.post[0], PostItem { name: "n".into(), value: String::new(), file: true });
        assert_eq!(o.get("load.post.count").as_deref(), Some("1"));
        assert!(o.set("load.post.delete", "0"));
        assert!(o.load.post.is_empty());
        assert!(!o.set("load.post[3].name", "x"));
    }

    #[test]
    fn object_first_matches_any_prefix() {
        let mut o = PdfObject::default();
        o.load.allowed = vec!["a".into(), "b".into()];
        assert_eq!(o.get("load.allowed.f").as_deref(), Some("a"));
        assert_eq!(o.get("load.allowed.la").as_deref(), Some("b"));
        assert_eq!(o.get("load.allowed.firsts"), None);
    }

    #[test]
    fn object_scalars() {
        let mut o = PdfObject::default();
        assert_eq!(o.get("toc.fontScale").as_deref(), Some("0.8"));
        assert_eq!(o.get("load.zoomFactor").as_deref(), Some("1"));
        assert_eq!(o.get("load.proxy").as_deref(), Some(""));
        assert!(!o.set("web.printMediaType", "true"));
        assert!(o.set("load.printMediaType", "true"));
        assert!(o.load.print_media_type);
        assert!(o.set("header.spacing", "1.5"));
        assert_eq!(o.get("header.spacing").as_deref(), Some("1.5"));
        assert!(o.set("load.loadErrorHandling", "skip"));
        assert_eq!(o.get("load.loadErrorHandling").as_deref(), Some("skip"));
        // mediaLoadErrorHandling is not reflected upstream.
        assert_eq!(o.get("load.mediaLoadErrorHandling"), None);
    }
}
