//! Page loading settings. Ports upstream `src/lib/loadsettings.hh` and
//! `src/lib/loadsettings.cc`.

use crate::qt;

/// `QNetworkProxy::ProxyType` values used by wkhtmltopdf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyType {
    #[default]
    NoProxy,
    HttpProxy,
    Socks5Proxy,
}

/// `settings::Proxy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub kind: ProxyType,
    pub port: i32,
    pub host: String,
    pub user: String,
    pub password: String,
}

impl Default for Proxy {
    fn default() -> Self {
        Proxy {
            kind: ProxyType::NoProxy,
            port: -1,
            host: String::new(),
            user: String::new(),
            password: String::new(),
        }
    }
}

/// `settings::PostItem`: one `--post` or `--post-file` field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostItem {
    pub name: String,
    pub value: String,
    pub file: bool,
}

/// `settings::LoadGlobal`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadGlobal {
    pub cookie_jar: String,
}

/// `LoadPage::LoadErrorHandling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadErrorHandling {
    #[default]
    Abort,
    Skip,
    Ignore,
}

/// `settings::LoadPage`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadPage {
    pub username: String,
    pub password: String,
    pub client_ssl_key_path: String,
    pub client_ssl_key_password: String,
    pub client_ssl_crt_path: String,
    pub jsdelay: i32,
    pub window_status: String,
    pub zoom_factor: f32,
    pub custom_headers: Vec<(String, String)>,
    pub repeat_custom_headers: bool,
    pub cookies: Vec<(String, String)>,
    pub post: Vec<PostItem>,
    pub block_local_file_access: bool,
    pub allowed: Vec<String>,
    pub stop_slow_scripts: bool,
    pub debug_javascript: bool,
    pub load_error_handling: LoadErrorHandling,
    pub media_load_error_handling: LoadErrorHandling,
    pub proxy: Proxy,
    pub run_script: Vec<String>,
    pub checkbox_svg: String,
    pub checkbox_checked_svg: String,
    pub radiobutton_svg: String,
    pub radiobutton_checked_svg: String,
    pub cache_dir: String,
    pub bypass_proxy_for_hosts: Vec<String>,
    pub proxy_host_name_lookup: bool,
    pub print_media_type: bool,
}

impl Default for LoadPage {
    fn default() -> Self {
        LoadPage {
            username: String::new(),
            password: String::new(),
            client_ssl_key_path: String::new(),
            client_ssl_key_password: String::new(),
            client_ssl_crt_path: String::new(),
            jsdelay: 200,
            window_status: String::new(),
            zoom_factor: 1.0,
            custom_headers: Vec::new(),
            repeat_custom_headers: false,
            cookies: Vec::new(),
            post: Vec::new(),
            block_local_file_access: true,
            allowed: Vec::new(),
            stop_slow_scripts: true,
            debug_javascript: false,
            load_error_handling: LoadErrorHandling::Abort,
            media_load_error_handling: LoadErrorHandling::Ignore,
            proxy: Proxy::default(),
            run_script: Vec::new(),
            checkbox_svg: String::new(),
            checkbox_checked_svg: String::new(),
            radiobutton_svg: String::new(),
            radiobutton_checked_svg: String::new(),
            cache_dir: String::new(),
            bypass_proxy_for_hosts: Vec::new(),
            proxy_host_name_lookup: false,
            print_media_type: false,
        }
    }
}

/// `LoadPage::mediaFilesExtensions`: failures of these resources are only
/// "media" errors (matched against `QFileInfo::completeSuffix`).
pub const MEDIA_FILES_EXTENSIONS: &[&str] = &["css", "js", "svg", "png", "jpg", "jpeg", "gif"];

/// `strToLoadErrorHandling`: case-insensitive `abort|skip|ignore`.
pub fn str_to_load_error_handling(s: &str) -> (LoadErrorHandling, bool) {
    if s.eq_ignore_ascii_case("abort") {
        (LoadErrorHandling::Abort, true)
    } else if s.eq_ignore_ascii_case("skip") {
        (LoadErrorHandling::Skip, true)
    } else if s.eq_ignore_ascii_case("ignore") {
        (LoadErrorHandling::Ignore, true)
    } else {
        (LoadErrorHandling::Abort, false)
    }
}

/// `loadErrorHandlingToStr`.
pub fn load_error_handling_to_str(l: LoadErrorHandling) -> &'static str {
    match l {
        LoadErrorHandling::Abort => "abort",
        LoadErrorHandling::Skip => "skip",
        LoadErrorHandling::Ignore => "ignore",
    }
}

/// `strToProxy`: parse `[http://|socks5://][user[:password]@]host[:port]`
/// or the literal `none` (lowercase only).
pub fn str_to_proxy(input: &str) -> (Proxy, bool) {
    let mut p = Proxy::default();
    let mut ok = true;
    if input == "none" {
        return (p, true);
    }
    p.kind = ProxyType::HttpProxy;
    let mut rest = input;
    if let Some(r) = rest.strip_prefix("http://") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("socks5://") {
        p.kind = ProxyType::Socks5Proxy;
        rest = r;
    }
    // The user part ends at the *last* '@'.
    if let Some(at) = rest.rfind('@') {
        let user = &rest[..at];
        rest = &rest[at + 1..];
        match user.find(':') {
            Some(idx) => {
                p.password = user[idx + 1..].to_string();
                p.user = user[..idx].to_string();
            }
            None => p.user = user.to_string(),
        }
    }
    p.port = 1080;
    match rest.find(':') {
        None => p.host = rest.to_string(),
        Some(idx) => {
            match qt::to_int(&rest[idx + 1..]) {
                Some(port) => p.port = port,
                None => {
                    // QString::toInt returns 0 on failure.
                    p.port = 0;
                    ok = false;
                }
            }
            if p.port < 0 || p.port > 65535 {
                p.port = 1080;
                ok = false;
            }
            p.host = rest[..idx].to_string();
        }
    }
    if p.host.is_empty() {
        ok = false;
    }
    (p, ok)
}

/// `proxyToStr`. Reproduces the upstream rendering, including the `@` placed
/// before the user name.
pub fn proxy_to_str(p: &Proxy) -> String {
    let mut res = String::new();
    match p.kind {
        ProxyType::HttpProxy => res.push_str("http://"),
        ProxyType::Socks5Proxy => res.push_str("socks5://"),
        ProxyType::NoProxy => {}
    }
    if !p.user.is_empty() {
        res.push('@');
        res.push_str(&p.user);
        if !p.password.is_empty() {
            res.push(':');
            res.push_str(&p.password);
        }
    }
    res.push_str(&p.host);
    if !p.host.is_empty() {
        res.push(':');
        res.push_str(&p.port.to_string());
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_none_is_lowercase_only() {
        let (p, ok) = str_to_proxy("none");
        assert!(ok);
        assert_eq!(p.kind, ProxyType::NoProxy);
        // "None" (as written in the help text) is parsed as a host name.
        let (p, ok) = str_to_proxy("None");
        assert!(ok);
        assert_eq!(p.kind, ProxyType::HttpProxy);
        assert_eq!(p.host, "None");
    }

    #[test]
    fn proxy_full_form() {
        let (p, ok) = str_to_proxy("socks5://us:er:pw@x@host:8080");
        assert!(ok);
        assert_eq!(p.kind, ProxyType::Socks5Proxy);
        assert_eq!(p.user, "us");
        assert_eq!(p.password, "er:pw@x");
        assert_eq!(p.host, "host");
        assert_eq!(p.port, 8080);
    }

    #[test]
    fn proxy_defaults_and_errors() {
        let (p, ok) = str_to_proxy("http://myproxy");
        assert!(ok);
        assert_eq!(p.port, 1080);
        assert!(!str_to_proxy("host:70000").1);
        assert!(!str_to_proxy("host:abc").1);
        assert!(!str_to_proxy(":8080").1);
        assert!(!str_to_proxy("").1);
    }

    #[test]
    fn proxy_to_str_puts_at_before_user() {
        let (p, _) = str_to_proxy("http://u:p@h:1");
        assert_eq!(proxy_to_str(&p), "http://@u:ph:1");
    }

    #[test]
    fn load_error_handling_values() {
        assert_eq!(str_to_load_error_handling("IGNORE"), (LoadErrorHandling::Ignore, true));
        assert_eq!(str_to_load_error_handling("Skip"), (LoadErrorHandling::Skip, true));
        assert_eq!(str_to_load_error_handling("nope"), (LoadErrorHandling::Abort, false));
    }
}
