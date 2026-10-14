//! Input name → URL resolution. Ports `MultiPageLoader::guessUrlFromString`
//! from upstream `src/lib/multipageloader.cc` (spec §3.2, quirk §10.6).

use std::path::Path;

fn is_host_with_port(s: &str) -> bool {
    // ^[a-zA-Z\.]+\:[0-9]*$
    match s.split_once(':') {
        Some((host, port)) => {
            !host.is_empty()
                && host.bytes().all(|b| b.is_ascii_alphabetic() || b == b'.')
                && port.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

fn has_schema(s: &str) -> bool {
    // ^[a-zA-Z]+\://.*
    match s.find("://") {
        Some(i) => i > 0 && s[..i].bytes().all(|b| b.is_ascii_alphabetic()),
        None => false,
    }
}

fn file_url(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    // `QUrl::fromLocalFile`. On Unix the path is kept as typed (not percent-encoded), which is
    // how upstream prints it in messages. Windows paths (`C:\…`, `\\?\C:\…`) need the URL
    // form `file:///C:/…`.
    if cfg!(windows) {
        if let Ok(u) = url::Url::from_file_path(&abs) {
            return u.to_string();
        }
    }
    format!("file://{}", abs.display())
}

/// `guessUrlFromString`. Notably, a local file name that does not exist and
/// contains a dot becomes `http://<name>`.
pub fn guess_url_from_string(string: &str) -> String {
    let mut url_str = string.trim().to_string();

    // A host with a port.
    if is_host_with_port(&url_str) {
        url_str = format!("http://{url_str}");
    }

    // Looks like a qualified URL.
    let schema = has_schema(&url_str);
    if schema {
        return url_str;
    }

    // Might be a file.
    let p = Path::new(&url_str);
    if !url_str.is_empty() && p.exists() {
        return file_url(p);
    }

    // Might be a short URL: guess the schema from the first label.
    if let Some(dot) = url_str.find('.') {
        let prefix = url_str[..dot].to_lowercase();
        let schema = if prefix == "ftp" { "ftp" } else { "http" };
        return format!("{schema}://{url_str}");
    }

    // Fall back to the tolerant parser; add http:// when there is no scheme.
    if string.contains(':') && has_schema_like_prefix(string) {
        string.to_string()
    } else {
        format!("http://{string}")
    }
}

/// Whether `QUrl` would see a scheme (`[a-zA-Z][a-zA-Z0-9+.-]*:`).
fn has_schema_like_prefix(s: &str) -> bool {
    let Some(i) = s.find(':') else { return false };
    let head = &s[..i];
    let mut b = head.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_alphabetic())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_input_file_becomes_http_url() {
        assert_eq!(guess_url_from_string("nonexist.html"), "http://nonexist.html");
        assert_eq!(guess_url_from_string("ftp.example.com"), "ftp://ftp.example.com");
    }

    #[test]
    fn existing_file_becomes_file_url() {
        let here = std::env::current_dir().unwrap().join("Cargo.toml");
        assert!(here.exists());
        let want = if cfg!(windows) {
            url::Url::from_file_path(&here).unwrap().to_string()
        } else {
            format!("file://{}", here.display())
        };
        assert_eq!(guess_url_from_string("Cargo.toml"), want);
    }

    #[test]
    fn urls_and_host_port() {
        assert_eq!(guess_url_from_string("https://example.com/a"), "https://example.com/a");
        assert_eq!(guess_url_from_string(" localhost:8080 "), "http://localhost:8080");
        assert_eq!(guess_url_from_string("intranet"), "http://intranet");
    }
}
