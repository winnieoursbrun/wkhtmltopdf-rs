//! Resource loading. Ports upstream `src/lib/multipageloader.cc`
//! (`MultiPageLoader`, `ResourceObject`, `MyNetworkAccessManager`,
//! `MyCookieJar`) on top of `ureq` + `rustls`, with the observable behaviour
//! of wkhtmltopdf 0.12.6 (spec §3.2, §4.3):
//!
//! - inputs: `-` (stdin copied to a `wktemp-<uuid>.html` temp file and loaded
//!   as `file://`), file paths, `file://`, `http(s)://`, `data:`;
//! - local file policy: `--disable/enable-local-file-access`, `--allow`;
//!   blocked requests are rewritten to `about:blank`, which fails with
//!   `ProtocolUnknownError` (exit 1);
//! - per-reply error accounting (`amfinished`) including the media-extension
//!   quirk (`completeSuffix`), feeding the exit code;
//! - cookies, custom headers (+ propagation), POST bodies, HTTP basic auth,
//!   proxies with `--bypass-proxy-for`.
//!
//! Loading is sequential and deterministic: resources are started in object
//! order, then loaded one after the other. `--javascript-delay` is not slept:
//! scripts do not run yet (P4) and the delay only changes timing, never the
//! output (deliberate divergence, DESIGN.md "Determinism").

pub mod cookies;
pub mod http;
pub mod qnetwork;
pub mod urlguess;

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use url::Url;
use wk_settings::{LoadErrorHandling, LoadGlobal, LoadPage};

pub use cookies::CookieJar;
pub use qnetwork::NetworkError;

/// Where the loader reports what it does. Implemented by the converter,
/// which forwards to `ProgressFeedback`.
pub trait Feedback {
    fn warning(&mut self, message: &str);
    fn error(&mut self, message: &str);
    /// `loadProgress(percent)` of the whole loader.
    fn progress(&mut self, percent: i32);
}

/// A finished request (`QNetworkReply` at `finished()`).
#[derive(Debug, Clone)]
pub struct Reply {
    /// The URL of the reply as displayed in messages (`about:blank` for a
    /// blocked request).
    pub url: Url,
    /// URL after redirects; relative references resolve against it.
    pub final_url: Url,
    pub error: NetworkError,
    pub error_string: String,
    /// HTTP status, 0 for non-HTTP replies.
    pub http_status: i32,
    pub body: Vec<u8>,
    /// `Content-Type` header, if any.
    pub content_type: Option<String>,
}

impl Reply {
    fn ok(url: Url, body: Vec<u8>, content_type: Option<String>) -> Reply {
        Reply {
            final_url: url.clone(),
            url,
            error: NetworkError::NoError,
            error_string: String::new(),
            http_status: 0,
            body,
            content_type,
        }
    }

    fn failed(url: Url, error: NetworkError, error_string: String) -> Reply {
        Reply {
            final_url: url.clone(),
            url,
            error,
            error_string,
            http_status: 0,
            body: Vec::new(),
            content_type: None,
        }
    }

    /// Whether the reply carries a usable body.
    pub fn is_success(&self) -> bool {
        self.error == NetworkError::NoError
    }
}

/// A request after `createRequest`: possibly rewritten to `about:blank`.
#[derive(Debug, Clone)]
pub struct PendingRequest {
    /// What the page asked for.
    pub requested: Url,
    /// What is actually fetched.
    pub effective: Url,
    /// Whether this is the page's main document.
    pub main: bool,
}

/// `QFileInfo(url.toString()).completeSuffix().toLower().remove(QRegExp("\\?.*$"))`.
pub fn complete_suffix(url_string: &str) -> String {
    let file_name = url_string.rsplit('/').next().unwrap_or("");
    let suffix = match file_name.find('.') {
        Some(i) => &file_name[i + 1..],
        None => "",
    };
    let suffix = suffix.to_lowercase();
    match suffix.find('?') {
        Some(i) => suffix[..i].to_string(),
        None => suffix,
    }
}

/// `QFileInfo::canonicalFilePath`: empty for missing files.
fn canonical(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

/// One page object being loaded (`ResourceObject` + its
/// `MyNetworkAccessManager`).
pub struct ResourceObject {
    /// The page URL.
    pub url: Url,
    /// The page URL as `QUrl::toString()` renders the input (no trailing
    /// slash is added to a bare host, unlike [`Url`]).
    pub url_display: String,
    pub settings: LoadPage,
    allowed: HashSet<PathBuf>,
    /// First recorded error code (network error + 1000, or HTTP status).
    pub http_error_code: i32,
    progress: i32,
    /// `lo.skip`: the object is dropped from the output.
    pub skip: bool,
    /// Whether the page load finished.
    pub finished: bool,
    /// A `--post-file` could not be read (`multiPageLoader.fail()`).
    pub post_failed: bool,
    http: http::HttpClient,
}

impl ResourceObject {
    fn new(url: Url, url_display: String, settings: LoadPage, user_agent: &str) -> ResourceObject {
        let mut allowed = HashSet::new();
        for p in &settings.allowed {
            if let Some(c) = canonical(Path::new(p)) {
                allowed.insert(c);
            }
        }
        if url.scheme() == "file" {
            if let Some(c) = url.to_file_path().ok().and_then(|p| canonical(&p)) {
                allowed.insert(c);
            }
        }
        let http = http::HttpClient::new(&settings, user_agent);
        ResourceObject {
            url,
            url_display,
            settings,
            allowed,
            http_error_code: 0,
            progress: 0,
            skip: false,
            finished: false,
            post_failed: false,
            http,
        }
    }

    fn is_allowed(&self, url: &Url) -> (bool, String) {
        let local = url.to_file_path().ok();
        let canon = local.as_deref().and_then(canonical);
        let shown = canon.as_ref().map(|c| c.display().to_string()).unwrap_or_default();
        let mut cur = canon.clone();
        while let Some(p) = cur {
            if self.allowed.contains(&p) {
                return (true, shown);
            }
            cur = p.parent().map(Path::to_path_buf);
        }
        (false, shown)
    }

    /// `MyNetworkAccessManager::createRequest`.
    pub fn create_request(&mut self, url: &Url, main: bool, fb: &mut dyn Feedback) -> PendingRequest {
        let scheme = url.scheme();
        let is_local = scheme.len() <= 1 || scheme == "file";
        let mut effective = url.clone();
        if is_local && self.settings.block_local_file_access {
            let (ok, shown) = self.is_allowed(url);
            if !ok {
                fb.warning(&format!("Blocked access to file {shown}"));
                effective = Url::parse("about:blank").expect("valid url");
            }
        }
        PendingRequest { requested: url.clone(), effective, main }
    }

    /// Perform the request, then account for its outcome (`amfinished`).
    pub fn finish_request(
        &mut self,
        req: &PendingRequest,
        jar: &mut CookieJar,
        fb: &mut dyn Feedback,
    ) -> Reply {
        let reply = self.fetch(req, jar, fb);
        self.account(&reply, fb);
        reply
    }

    fn fetch(&mut self, req: &PendingRequest, jar: &mut CookieJar, fb: &mut dyn Feedback) -> Reply {
        let url = req.effective.clone();
        match url.scheme() {
            "about" => {
                Reply::failed(url, NetworkError::ProtocolUnknownError, "Protocol \"about\" is unknown".into())
            }
            "data" => match data_url::DataUrl::process(url.as_str()) {
                Ok(d) => {
                    let mime = d.mime_type().to_string();
                    match d.decode_to_vec() {
                        Ok((body, _)) => Reply::ok(url, body, Some(mime)),
                        Err(_) => Reply::ok(url, Vec::new(), Some(mime)),
                    }
                }
                Err(_) => Reply::ok(url, Vec::new(), None),
            },
            "file" => {
                let path = url.to_file_path().unwrap_or_default();
                if path.is_dir() {
                    let s = format!("Cannot open {}: Path is a directory", qnetwork::qurl_to_string(&url));
                    return Reply::failed(url, NetworkError::ContentOperationNotPermittedError, s);
                }
                match std::fs::read(&path) {
                    Ok(body) => Reply::ok(url, body, None),
                    Err(e) => {
                        let reason = match e.kind() {
                            std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
                            std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                            _ => e.to_string(),
                        };
                        let s = format!("Error opening {}: {reason}", qnetwork::qurl_to_string(&url));
                        let err = if e.kind() == std::io::ErrorKind::PermissionDenied {
                            NetworkError::ContentAccessDenied
                        } else {
                            NetworkError::ContentNotFoundError
                        };
                        Reply::failed(url, err, s)
                    }
                }
            }
            "http" | "https" => {
                let main = req.main;
                let mut headers: Vec<(String, String)> = Vec::new();
                if main || self.settings.repeat_custom_headers {
                    headers.extend(self.settings.custom_headers.iter().cloned());
                }
                let post = if main { http::post_body(&self.settings, fb) } else { Ok(None) };
                let post = match post {
                    Ok(p) => p,
                    Err(http::PostFileError) => {
                        self.post_failed = true;
                        None
                    }
                };
                self.http.fetch(&url, main, &headers, post, jar, fb)
            }
            other => {
                let s = format!("Protocol \"{other}\" is unknown");
                Reply::failed(url, NetworkError::ProtocolUnknownError, s)
            }
        }
    }

    /// `ResourceObject::amfinished`: report the failure and record the
    /// error code.
    fn account(&mut self, reply: &Reply, fb: &mut dyn Feedback) {
        let network_status = reply.error.code();
        let http_status = reply.http_status;
        if !((network_status != 0 && network_status != 5) || (http_status > 399 && self.http_error_code == 0))
        {
            return;
        }
        let shown = qnetwork::qurl_to_string(&reply.url);
        let extension = complete_suffix(&shown);
        let media = wk_settings::loadsettings::MEDIA_FILES_EXTENSIONS.contains(&extension.as_str());
        let code = if network_status > 0 { network_status + 1000 } else { http_status };
        if !media {
            fb.error(&format!(
                "Failed to load {shown}, with network status code {network_status} and http status code {http_status} - {}",
                reply.error_string
            ));
            self.http_error_code = code;
            return;
        }
        if self.settings.media_load_error_handling == LoadErrorHandling::Abort {
            self.http_error_code = code;
            fb.error(&format!(
                "Failed to load {shown}, with code: {code} (sometimes it will work just to ignore this error with --load-media-error-handling ignore)"
            ));
        } else {
            fb.warning(&format!(
                "Failed to load {shown} ({})",
                wk_settings::loadsettings::load_error_handling_to_str(
                    self.settings.media_load_error_handling
                )
            ));
        }
    }
}

/// `MultiPageLoader`: a set of resources loaded together, with one cookie
/// jar and one progress counter.
pub struct MultiPageLoader {
    pub is_main_loader: bool,
    pub resources: Vec<ResourceObject>,
    pub jar: CookieJar,
    cookie_jar_path: String,
    progress_sum: i32,
    /// `hasError`: an abort-mode page failure happened.
    pub has_error: bool,
    temp_files: Vec<PathBuf>,
    user_agent: String,
}

/// The `User-Agent` QtWebKit 2.2 sends; the application name is the
/// executable's file name (verified: `wkhtmltopdf-linux-amd64`).
pub fn user_agent(app_name: &str) -> String {
    format!("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/534.34 (KHTML, like Gecko) {app_name} Safari/534.34")
}

/// A fresh `wktemp-<uuid>` name in the temp directory.
pub fn temp_path(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("wktemp-{}{ext}", uuid_v4()))
}

/// A random UUID (`QUuid::createUuid`), 36 characters.
pub fn uuid_v4() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.write_u32(std::process::id());
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

impl MultiPageLoader {
    pub fn new(global: &LoadGlobal, is_main_loader: bool, app_name: &str) -> MultiPageLoader {
        let jar = if global.cookie_jar.is_empty() {
            CookieJar::default()
        } else {
            CookieJar::load_from_file(&global.cookie_jar)
        };
        MultiPageLoader {
            is_main_loader,
            resources: Vec::new(),
            jar,
            cookie_jar_path: global.cookie_jar.clone(),
            progress_sum: 0,
            has_error: false,
            temp_files: Vec::new(),
            user_agent: user_agent(app_name),
        }
    }

    /// `MultiPageLoader::addResource(string, settings, data)`: `-` copies
    /// `stdin` to a temp file first. Returns the resource index, or the
    /// error message (`Unable to create temporary file`).
    pub fn add_resource(
        &mut self,
        input: &str,
        settings: &LoadPage,
        stdin: &mut dyn Read,
    ) -> Result<usize, String> {
        let mut name = input.to_string();
        if input == "-" {
            let path = temp_path(".html");
            let mut data = Vec::new();
            let ok = stdin.read_to_end(&mut data).is_ok() && std::fs::write(&path, &data).is_ok();
            if !ok {
                return Err("Unable to create temporary file".into());
            }
            self.temp_files.push(path.clone());
            name = path.display().to_string();
        }
        let guessed = urlguess::guess_url_from_string(&name);
        let url = Url::parse(&guessed)
            .or_else(|_| Url::parse(&format!("http://{guessed}")))
            .unwrap_or_else(|_| Url::parse("about:blank").expect("valid url"));
        let ro = ResourceObject::new(url, guessed, settings.clone(), &self.user_agent);
        self.resources.push(ro);
        Ok(self.resources.len() - 1)
    }

    /// `ResourceObject::loadProgress(p)` for resource `i`.
    pub fn set_progress(&mut self, i: usize, p: i32, fb: &mut dyn Feedback) {
        let n = self.resources.len() as i32;
        let r = &mut self.resources[i];
        if r.finished {
            return;
        }
        self.progress_sum -= r.progress;
        r.progress = p;
        self.progress_sum += p;
        fb.progress(self.progress_sum / n);
    }

    /// Fetch the main document of resource `i` (cookies, custom headers and
    /// POST body applied). The page load fails only on network errors: an
    /// HTTP error page is displayed like a normal page (QtWebKit's
    /// `ignoreHttpError`), but its error code is still recorded.
    pub fn load_main(&mut self, i: usize, fb: &mut dyn Feedback) -> (bool, Reply) {
        self.jar.clear_extra_cookies();
        for (n, v) in self.resources[i].settings.cookies.clone() {
            self.jar.use_cookie(&n, &v);
        }
        let url = self.resources[i].url.clone();
        let r = &mut self.resources[i];
        let pending = r.create_request(&url, true, fb);
        let reply = r.finish_request(&pending, &mut self.jar, fb);
        self.has_error |= r.post_failed;
        let ok = reply.http_status > 0 || reply.is_success();
        (ok, reply)
    }

    /// Sub-resource request of page `i`: `createRequest`.
    pub fn create_request(&mut self, i: usize, url: &Url, fb: &mut dyn Feedback) -> PendingRequest {
        self.resources[i].create_request(url, false, fb)
    }

    /// Sub-resource request of page `i`: fetch and `amfinished`.
    pub fn finish_request(&mut self, i: usize, req: &PendingRequest, fb: &mut dyn Feedback) -> Reply {
        self.resources[i].finish_request(req, &mut self.jar, fb)
    }

    /// `ResourceObject::loadFinished(ok)`.
    pub fn load_finished(&mut self, i: usize, ok: bool, fb: &mut dyn Feedback) {
        let r = &mut self.resources[i];
        self.has_error |= !ok && r.settings.load_error_handling == LoadErrorHandling::Abort;
        if !ok {
            match r.settings.load_error_handling {
                LoadErrorHandling::Abort => fb.error(&format!(
                    "Failed loading page {} (sometimes it will work just to ignore this error with --load-error-handling ignore)",
                    r.url_display
                )),
                LoadErrorHandling::Skip => {
                    fb.warning(&format!("Failed loading page {} (skipped)", r.url_display));
                    r.skip = true;
                }
                LoadErrorHandling::Ignore => {
                    fb.warning(&format!("Failed loading page {} (ignored)", r.url_display))
                }
            }
        }
        r.finished = true;
    }

    /// `loadDone`: save the cookie jar.
    pub fn load_done(&mut self) {
        if !self.cookie_jar_path.is_empty() {
            self.jar.save_to_file(&self.cookie_jar_path);
        }
    }

    /// `MultiPageLoader::httpErrorCode`: the maximum over resources.
    pub fn http_error_code(&self) -> i32 {
        self.resources.iter().map(|r| r.http_error_code).max().unwrap_or(0)
    }

    /// Remove the temp files (`clearResources`).
    pub fn clear_resources(&mut self) {
        for f in self.temp_files.drain(..) {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl Drop for MultiPageLoader {
    fn drop(&mut self) {
        self.clear_resources();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_suffix_quirk() {
        assert_eq!(complete_suffix("http://x/jquery.min.js"), "min.js");
        assert_eq!(complete_suffix("http://x/jquery.js"), "js");
        assert_eq!(complete_suffix("http://x/a.CSS?v=1.2"), "css");
        assert_eq!(complete_suffix("http://x.y/dir.d/file"), "");
        assert_eq!(complete_suffix("about:blank"), "");
    }

    struct Rec(Vec<String>);
    impl Feedback for Rec {
        fn warning(&mut self, m: &str) {
            self.0.push(format!("W:{m}"));
        }
        fn error(&mut self, m: &str) {
            self.0.push(format!("E:{m}"));
        }
        fn progress(&mut self, p: i32) {
            self.0.push(format!("P:{p}"));
        }
    }

    #[test]
    fn blocked_file_becomes_about_blank_error() {
        let mut l = MultiPageLoader::new(&LoadGlobal::default(), true, "wkhtmltopdf");
        let i = l.add_resource("data:text/html,x", &LoadPage::default(), &mut std::io::empty()).unwrap();
        let mut fb = Rec(vec![]);
        let u = Url::parse("file:///nonexistent-dir/x.css").unwrap();
        let p = l.create_request(i, &u, &mut fb);
        let r = l.finish_request(i, &p, &mut fb);
        assert!(!r.is_success());
        assert_eq!(
            fb.0,
            vec![
                "W:Blocked access to file ".to_string(),
                "E:Failed to load about:blank, with network status code 301 and http status code 0 - Protocol \"about\" is unknown".to_string()
            ]
        );
        assert_eq!(l.http_error_code(), 1301);
    }

    #[test]
    fn missing_media_file_with_access_is_a_warning() {
        let mut l = MultiPageLoader::new(&LoadGlobal::default(), true, "wkhtmltopdf");
        let s = LoadPage { block_local_file_access: false, ..LoadPage::default() };
        let i = l.add_resource("data:text/html,x", &s, &mut std::io::empty()).unwrap();
        let mut fb = Rec(vec![]);
        let u = Url::parse("file:///nonexistent-dir/missing.css").unwrap();
        let p = l.create_request(i, &u, &mut fb);
        l.finish_request(i, &p, &mut fb);
        assert_eq!(fb.0, vec!["W:Failed to load file:///nonexistent-dir/missing.css (ignore)".to_string()]);
        assert_eq!(l.http_error_code(), 0);
    }
}
