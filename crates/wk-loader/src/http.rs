//! HTTP(S) transport standing in for `QNetworkAccessManager`: proxies
//! (`--proxy`, `--bypass-proxy-for`), POST bodies (`ResourceObject::load`),
//! HTTP authentication (`handleAuthenticationRequired`), cookies and the
//! Qt 4.8 mapping of failures to `QNetworkReply::NetworkError`.
//!
//! TLS certificate errors are ignored like upstream (`ignoreSslErrors`), but
//! the per-reply `SSL error ignored` warning is not reproduced yet, and
//! `--ssl-crt-path`/`--ssl-key-path` client certificates and `--cache-dir`
//! are accepted but not used (TODO P2b).

use std::io::Read;

use base64::Engine;
use ureq::Agent;
use ureq::ResponseExt;
use ureq::tls::TlsConfig;
use url::Url;
use wk_settings::{LoadPage, ProxyType};

use crate::cookies::CookieJar;
use crate::qnetwork::{NetworkError, status_code_from_http};
use crate::{Feedback, Reply};

/// The `Accept` header QtWebKit sends for documents.
const ACCEPT_DOCUMENT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

pub struct HttpClient {
    proxied: Agent,
    direct: Agent,
    bypass: Vec<String>,
    username: String,
    password: String,
    login_try: usize,
}

fn agent(proxy: Option<ureq::Proxy>, user_agent: &str) -> Agent {
    let config = Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(20)
        .proxy(proxy)
        .user_agent(user_agent)
        .tls_config(TlsConfig::builder().disable_verification(true).build())
        .timeout_connect(Some(std::time::Duration::from_secs(60)))
        .build();
    config.into()
}

fn proxy_of(settings: &LoadPage) -> Option<ureq::Proxy> {
    let p = &settings.proxy;
    if p.host.is_empty() || p.kind == ProxyType::NoProxy {
        return None;
    }
    let scheme = match p.kind {
        ProxyType::Socks5Proxy => "socks5",
        _ => "http",
    };
    let auth = if p.user.is_empty() {
        String::new()
    } else if p.password.is_empty() {
        format!("{}@", p.user)
    } else {
        format!("{}:{}@", p.user, p.password)
    };
    ureq::Proxy::new(&format!("{scheme}://{auth}{}:{}", p.host, p.port)).ok()
}

/// The POST body of the main request and its content type, built like
/// `ResourceObject::load`. `Err` when a `--post-file` cannot be read (the
/// error is reported and the loader fails).
/// A `--post-file` could not be read (already reported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostFileError;

/// A POST body and its content type.
pub type PostBody = (Vec<u8>, String);

pub fn post_body(settings: &LoadPage, fb: &mut dyn Feedback) -> Result<Option<PostBody>, PostFileError> {
    if settings.post.is_empty() {
        return Ok(None);
    }
    let has_files = settings.post.iter().any(|p| p.file);
    if has_files {
        let boundary = crate::uuid_v4().replace('-', "");
        let mut data = Vec::new();
        let mut failed = false;
        for pi in &settings.post {
            data.extend_from_slice(b"--");
            data.extend_from_slice(boundary.as_bytes());
            data.extend_from_slice(b"\ncontent-disposition: form-data; name=\"");
            data.extend_from_slice(pi.name.as_bytes());
            data.push(b'"');
            if pi.file {
                let content = std::fs::File::open(&pi.value).and_then(|mut f| {
                    let mut v = Vec::new();
                    f.read_to_end(&mut v).map(|_| v)
                });
                let content = match content {
                    Ok(c) => c,
                    Err(_) => {
                        fb.error(&format!("Unable to open file {}", pi.value));
                        failed = true;
                        Vec::new()
                    }
                };
                let name = std::path::Path::new(&pi.value)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                data.extend_from_slice(b"; filename=\"");
                data.extend_from_slice(name.as_bytes());
                data.extend_from_slice(b"\"\n\n");
                data.extend_from_slice(&content);
            } else {
                data.extend_from_slice(b"\n\n");
                data.extend_from_slice(pi.value.as_bytes());
            }
            data.push(b'\n');
        }
        data.extend_from_slice(b"--");
        data.extend_from_slice(boundary.as_bytes());
        data.extend_from_slice(b"--\n");
        if failed {
            return Err(PostFileError);
        }
        return Ok(Some((data, format!("multipart/form-data, boundary={boundary}"))));
    }
    // Qt 4 `QUrl::addQueryItem` + `encodedQuery`.
    let body: Vec<String> = settings
        .post
        .iter()
        .map(|p| format!("{}={}", query_encode(&p.name), query_encode(&p.value)))
        .collect();
    let body = body.join("&");
    if body.is_empty() {
        return Ok(None);
    }
    Ok(Some((body.into_bytes(), "application/x-www-form-urlencoded".into())))
}

/// Percent-encoding of `QUrl::addQueryItem` (Qt 4): `&`, `=`, `+`, `#`, `%`
/// and non-ASCII/space are encoded; `:` and `/` are kept.
pub fn query_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~!$'()*,;:@/?".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn map_error(e: &ureq::Error, url: &Url) -> (NetworkError, String) {
    let host = url.host_str().unwrap_or("").to_string();
    match e {
        ureq::Error::HostNotFound => (NetworkError::HostNotFoundError, format!("Host {host} not found")),
        ureq::Error::ConnectionFailed => (NetworkError::ConnectionRefusedError, "Connection refused".into()),
        ureq::Error::Timeout(_) => (NetworkError::TimeoutError, "Socket operation timed out".into()),
        ureq::Error::Tls(_) => (NetworkError::SslHandshakeFailedError, "SSL handshake failed".into()),
        ureq::Error::ConnectProxyFailed(_) | ureq::Error::InvalidProxyUrl => {
            (NetworkError::ProxyConnectionRefusedError, "Proxy connection refused".into())
        }
        ureq::Error::Io(io) if is_resolution_failure(io) => {
            (NetworkError::HostNotFoundError, format!("Host {host} not found"))
        }
        ureq::Error::Io(io) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused => {
                (NetworkError::ConnectionRefusedError, "Connection refused".into())
            }
            std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::BrokenPipe => {
                (NetworkError::RemoteHostClosedError, "Connection closed".into())
            }
            std::io::ErrorKind::TimedOut => (NetworkError::TimeoutError, "Socket operation timed out".into()),
            std::io::ErrorKind::NotFound => {
                (NetworkError::HostNotFoundError, format!("Host {host} not found"))
            }
            _ => (NetworkError::UnknownNetworkError, io.to_string()),
        },
        other => (NetworkError::UnknownNetworkError, other.to_string()),
    }
}

/// Name resolution failures come from `getaddrinfo` as uncategorised I/O
/// errors (`failed to lookup address information: …`). On Windows they are
/// Winsock errors instead (`No such host is known. (os error 11001)`).
fn is_resolution_failure(e: &std::io::Error) -> bool {
    if cfg!(windows) && is_winsock_resolution_error(e.raw_os_error()) {
        return true;
    }
    let m = e.to_string().to_ascii_lowercase();
    m.contains("lookup address")
        || m.contains("not known")
        || m.contains("name resolution")
        || m.contains("no address associated")
}

/// Winsock resolver errors that `QHostInfo` reports as `HostNotFound`:
/// `WSAHOST_NOT_FOUND` (11001), `WSATRY_AGAIN` (11002, no answer from the
/// DNS server, e.g. offline) and `WSANO_DATA` (11004, the name has no
/// address). Only meaningful for Windows error codes.
fn is_winsock_resolution_error(code: Option<i32>) -> bool {
    matches!(code, Some(11001 | 11002 | 11004))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winsock_resolution_errors() {
        assert!(is_winsock_resolution_error(Some(11001)));
        assert!(is_winsock_resolution_error(Some(11002)));
        assert!(is_winsock_resolution_error(Some(11004)));
        assert!(!is_winsock_resolution_error(Some(11003)));
        assert!(!is_winsock_resolution_error(Some(10061)));
        assert!(!is_winsock_resolution_error(None));
    }

    #[test]
    fn resolution_failure_maps_to_host_not_found() {
        let url = Url::parse("http://nonexist.html/").unwrap();
        let io = std::io::Error::other(
            "failed to lookup address information: nodename nor servname provided, or not known",
        );
        let (err, msg) = map_error(&ureq::Error::Io(io), &url);
        assert_eq!(err, NetworkError::HostNotFoundError);
        assert_eq!(msg, "Host nonexist.html not found");
    }

    #[cfg(windows)]
    #[test]
    fn windows_resolution_failure_maps_to_host_not_found() {
        let url = Url::parse("http://nonexistent.invalid/").unwrap();
        for code in [11001, 11002, 11004] {
            let io = std::io::Error::from_raw_os_error(code);
            let (err, msg) = map_error(&ureq::Error::Io(io), &url);
            assert_eq!(err, NetworkError::HostNotFoundError, "os error {code}");
            assert_eq!(msg, "Host nonexistent.invalid not found");
        }
    }
}

impl HttpClient {
    pub fn new(settings: &LoadPage, user_agent: &str) -> HttpClient {
        HttpClient {
            proxied: agent(proxy_of(settings), user_agent),
            direct: agent(None, user_agent),
            bypass: settings.bypass_proxy_for_hosts.iter().map(|h| h.to_lowercase()).collect(),
            username: settings.username.clone(),
            password: settings.password.clone(),
            login_try: 0,
        }
    }

    fn agent_for(&self, url: &Url) -> &Agent {
        let host = url.host_str().unwrap_or("").to_lowercase();
        if self.bypass.contains(&host) { &self.direct } else { &self.proxied }
    }

    /// One request, following redirects, answering authentication
    /// challenges at most twice.
    pub fn fetch(
        &mut self,
        url: &Url,
        main: bool,
        headers: &[(String, String)],
        post: Option<(Vec<u8>, String)>,
        jar: &mut CookieJar,
        fb: &mut dyn Feedback,
    ) -> Reply {
        let mut auth: Option<String> = None;
        loop {
            let agent = self.agent_for(url).clone();
            let mut builder = http_request(url, post.is_some());
            builder = builder.header("Accept", if main { ACCEPT_DOCUMENT } else { "*/*" });
            builder = builder.header("Accept-Language", "en,*");
            if let Some(c) = jar.header_for_url(url) {
                builder = builder.header("Cookie", c);
            }
            for (k, v) in headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            if let Some(a) = &auth {
                builder = builder.header("Authorization", a.as_str());
            }
            if let Some((_, ct)) = &post {
                builder = builder.header("Content-Type", ct.as_str());
            }
            // A GET has no body (and no `Content-Length`).
            let result = match &post {
                Some((body, _)) => builder.body(body.clone()).map(|r| agent.run(r)),
                None => builder.body(()).map(|r| agent.run(r)),
            };
            let result = match result {
                Ok(r) => r,
                Err(e) => {
                    return Reply::failed(url.clone(), NetworkError::ProtocolFailure, e.to_string());
                }
            };
            let mut resp = match result {
                Ok(r) => r,
                Err(e) => {
                    let (err, s) = map_error(&e, url);
                    return Reply::failed(url.clone(), err, s);
                }
            };
            let status = resp.status().as_u16();
            let final_url = Url::parse(&resp.get_uri().to_string()).unwrap_or_else(|_| url.clone());
            let set_cookie: Vec<String> = resp
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok().map(str::to_string))
                .collect();
            jar.set_cookies_from_url(&final_url, &set_cookie);
            let content_type =
                resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
            let challenge = resp.headers().contains_key("www-authenticate");
            let body = resp.body_mut().with_config().limit(u64::MAX).read_to_vec().unwrap_or_default();

            if status == 401 && challenge {
                // handleAuthenticationRequired
                if self.username.is_empty() {
                    fb.error("Authentication Required");
                } else if self.login_try >= 2 {
                    fb.error("Invalid username or password");
                } else {
                    self.login_try += 1;
                    let token = base64::engine::general_purpose::STANDARD
                        .encode(format!("{}:{}", self.username, self.password));
                    auth = Some(format!("Basic {token}"));
                    continue;
                }
                let mut r = Reply::failed(
                    url.clone(),
                    NetworkError::AuthenticationRequiredError,
                    "Host requires authentication".into(),
                );
                r.http_status = 401;
                r.final_url = final_url;
                r.body = body;
                r.content_type = content_type;
                return r;
            }
            let mut reply = Reply::ok(url.clone(), body, content_type);
            reply.final_url = final_url;
            reply.http_status = i32::from(status);
            if status >= 400 {
                reply.error = status_code_from_http(status);
                let reason = resp.status().canonical_reason().unwrap_or("");
                reply.error_string = format!(
                    "Error downloading {} - server replied: {reason}",
                    crate::qnetwork::qurl_to_string(url)
                );
            }
            return reply;
        }
    }
}

fn http_request(url: &Url, post: bool) -> ureq::http::request::Builder {
    ureq::http::Request::builder().method(if post { "POST" } else { "GET" }).uri(url.as_str())
}
