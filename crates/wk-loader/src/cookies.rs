//! Cookie handling. Ports `MyCookieJar` from upstream
//! `src/lib/multipageloader.cc` over a minimal `QNetworkCookieJar`.
//!
//! - `--cookie name value` cookies are appended to every request, whatever
//!   its domain (`cookiesForUrl` appends the "extra" cookies unconditionally);
//!   values are sent raw.
//! - `--cookie-jar` is read with `QNetworkCookie::parseCookies` semantics
//!   (one `Set-Cookie`-like line per cookie) and written back as
//!   `toRawForm() + ";\n"` per cookie. Verified on 0.12.6: a `Set-Cookie:
//!   sid=abc; Path=/` from host `h` is saved as `sid=abc; domain=h; path=/;`.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<String>,
    pub secure: bool,
    pub http_only: bool,
}

impl Cookie {
    /// `QNetworkCookie::toRawForm(Full)`.
    pub fn to_raw_form(&self) -> String {
        let mut s = format!("{}={}", self.name, self.value);
        if self.secure {
            s.push_str("; secure");
        }
        if self.http_only {
            s.push_str("; HttpOnly");
        }
        if let Some(e) = &self.expires {
            s.push_str("; expires=");
            s.push_str(e);
        }
        if !self.domain.is_empty() {
            s.push_str("; domain=");
            s.push_str(&self.domain);
        }
        if !self.path.is_empty() {
            s.push_str("; path=");
            s.push_str(&self.path);
        }
        s
    }
}

/// Parse one `Set-Cookie` value. `default_host` and `default_path` fill the
/// domain and path like `QNetworkCookieJar::setCookiesFromUrl`.
pub fn parse_set_cookie(line: &str, default_host: &str, default_path: &str) -> Option<Cookie> {
    let mut parts = line.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut c = Cookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        domain: default_host.to_string(),
        path: default_path.to_string(),
        expires: None,
        secure: false,
        http_only: false,
    };
    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (k, v) = attr.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((attr, ""));
        match k.to_ascii_lowercase().as_str() {
            "domain" if !v.is_empty() => {
                c.domain = if v.starts_with('.') || v == default_host || default_host.is_empty() {
                    v.to_string()
                } else {
                    format!(".{v}")
                }
            }
            "path" if !v.is_empty() => c.path = v.to_string(),
            "expires" => c.expires = Some(v.to_string()),
            "secure" => c.secure = true,
            "httponly" => c.http_only = true,
            _ => {}
        }
    }
    Some(c)
}

fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let d = cookie_domain.to_ascii_lowercase();
    match d.strip_prefix('.') {
        Some(base) => host == base || host.ends_with(&d),
        None => host == d,
    }
}

fn path_matches(cookie_path: &str, path: &str) -> bool {
    cookie_path.is_empty()
        || path == cookie_path
        || (path.starts_with(cookie_path)
            && (cookie_path.ends_with('/') || path[cookie_path.len()..].starts_with('/')))
}

/// `MyCookieJar`.
#[derive(Debug, Default, Clone)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
    extra: Vec<(String, String)>,
}

impl CookieJar {
    /// `loadFromFile`: a missing or unreadable file gives an empty jar.
    pub fn load_from_file(path: &str) -> CookieJar {
        let mut jar = CookieJar::default();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if let Some(c) = parse_set_cookie(line, "", "") {
                    jar.insert(c);
                }
            }
        }
        jar
    }

    /// `saveToFile`.
    pub fn save_to_file(&self, path: &str) {
        let mut out = String::new();
        for c in &self.cookies {
            out.push_str(&c.to_raw_form());
            out.push_str(";\n");
        }
        let _ = std::fs::write(Path::new(path), out);
    }

    fn insert(&mut self, c: Cookie) {
        self.cookies.retain(|o| !(o.name == c.name && o.domain == c.domain && o.path == c.path));
        self.cookies.push(c);
    }

    /// `setCookiesFromUrl` for the `Set-Cookie` headers of a reply.
    pub fn set_cookies_from_url(&mut self, url: &url::Url, set_cookie: &[String]) {
        let host = url.host_str().unwrap_or("");
        let path = url.path();
        let default_path = match path.rfind('/') {
            Some(0) | None => "/".to_string(),
            Some(i) => path[..i].to_string(),
        };
        for line in set_cookie {
            if let Some(c) = parse_set_cookie(line, host, &default_path) {
                self.insert(c);
            }
        }
    }

    /// `clearExtraCookies`.
    pub fn clear_extra_cookies(&mut self) {
        self.extra.clear();
    }

    /// `useCookie`.
    pub fn use_cookie(&mut self, name: &str, value: &str) {
        self.extra.push((name.to_string(), value.to_string()));
    }

    /// The `Cookie:` header value for `url` (`cookiesForUrl`), or `None`.
    pub fn header_for_url(&self, url: &url::Url) -> Option<String> {
        let host = url.host_str().unwrap_or("");
        let secure = url.scheme() == "https";
        let mut parts: Vec<String> = self
            .cookies
            .iter()
            .filter(|c| domain_matches(&c.domain, host) && path_matches(&c.path, url.path()))
            .filter(|c| secure || !c.secure)
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        parts.extend(self.extra.iter().map(|(n, v)| format!("{n}={v}")));
        if parts.is_empty() { None } else { Some(parts.join("; ")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_form_like_qt() {
        let mut jar = CookieJar::default();
        let u = url::Url::parse("http://h:8765/hdr").unwrap();
        jar.set_cookies_from_url(&u, &["sid=abc; Path=/".to_string()]);
        assert_eq!(jar.cookies[0].to_raw_form() + ";", "sid=abc; domain=h; path=/;");
        jar.use_cookie("a", "b%20c");
        assert_eq!(jar.header_for_url(&u).unwrap(), "sid=abc; a=b%20c");
        let other = url::Url::parse("http://other/").unwrap();
        assert_eq!(jar.header_for_url(&other).unwrap(), "a=b%20c");
    }

    #[test]
    fn jar_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("wk-jar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("jar.txt");
        std::fs::write(&f, "sid=abc; domain=h; path=/;\n").unwrap();
        let jar = CookieJar::load_from_file(f.to_str().unwrap());
        let u = url::Url::parse("http://h/x").unwrap();
        assert_eq!(jar.header_for_url(&u).unwrap(), "sid=abc");
        jar.save_to_file(f.to_str().unwrap());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "sid=abc; domain=h; path=/;\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
