//! The parts of Qt 4.8's `QNetworkReply` that leak into wkhtmltopdf's output:
//! the `NetworkError` codes and enum key names (printed by `handleError`), the
//! mapping of HTTP statuses to network errors (`statusCodeFromHttp` in
//! `qnetworkaccesshttpbackend.cpp`) and the error strings.
//!
//! Verified against the 0.12.6 binary: an HTTP 404 sub-resource reports
//! `network status code 203 and http status code 404 - Error downloading <url>
//! - server replied: <reason>` and the process ends with
//! `Exit with code 1 due to network error: ContentNotFoundError`.

/// `QNetworkReply::NetworkError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkError {
    NoError = 0,
    ConnectionRefusedError = 1,
    RemoteHostClosedError = 2,
    HostNotFoundError = 3,
    TimeoutError = 4,
    OperationCanceledError = 5,
    SslHandshakeFailedError = 6,
    TemporaryNetworkFailureError = 7,
    UnknownNetworkError = 99,
    ProxyConnectionRefusedError = 101,
    ProxyConnectionClosedError = 102,
    ProxyNotFoundError = 103,
    ProxyTimeoutError = 104,
    ProxyAuthenticationRequiredError = 105,
    UnknownProxyError = 199,
    ContentAccessDenied = 201,
    ContentOperationNotPermittedError = 202,
    ContentNotFoundError = 203,
    AuthenticationRequiredError = 204,
    ContentReSendError = 205,
    UnknownContentError = 299,
    ProtocolUnknownError = 301,
    ProtocolInvalidOperationError = 302,
    ProtocolFailure = 399,
}

const ALL: &[NetworkError] = {
    use NetworkError::*;
    &[
        NoError,
        ConnectionRefusedError,
        RemoteHostClosedError,
        HostNotFoundError,
        TimeoutError,
        OperationCanceledError,
        SslHandshakeFailedError,
        TemporaryNetworkFailureError,
        UnknownNetworkError,
        ProxyConnectionRefusedError,
        ProxyConnectionClosedError,
        ProxyNotFoundError,
        ProxyTimeoutError,
        ProxyAuthenticationRequiredError,
        UnknownProxyError,
        ContentAccessDenied,
        ContentOperationNotPermittedError,
        ContentNotFoundError,
        AuthenticationRequiredError,
        ContentReSendError,
        UnknownContentError,
        ProtocolUnknownError,
        ProtocolInvalidOperationError,
        ProtocolFailure,
    ]
};

impl NetworkError {
    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn from_code(code: i32) -> Option<NetworkError> {
        ALL.iter().copied().find(|e| e.code() == code)
    }

    /// `QMetaEnum::valueToKey`.
    pub fn key(self) -> &'static str {
        use NetworkError::*;
        match self {
            NoError => "NoError",
            ConnectionRefusedError => "ConnectionRefusedError",
            RemoteHostClosedError => "RemoteHostClosedError",
            HostNotFoundError => "HostNotFoundError",
            TimeoutError => "TimeoutError",
            OperationCanceledError => "OperationCanceledError",
            SslHandshakeFailedError => "SslHandshakeFailedError",
            TemporaryNetworkFailureError => "TemporaryNetworkFailureError",
            UnknownNetworkError => "UnknownNetworkError",
            ProxyConnectionRefusedError => "ProxyConnectionRefusedError",
            ProxyConnectionClosedError => "ProxyConnectionClosedError",
            ProxyNotFoundError => "ProxyNotFoundError",
            ProxyTimeoutError => "ProxyTimeoutError",
            ProxyAuthenticationRequiredError => "ProxyAuthenticationRequiredError",
            UnknownProxyError => "UnknownProxyError",
            ContentAccessDenied => "ContentAccessDenied",
            ContentOperationNotPermittedError => "ContentOperationNotPermittedError",
            ContentNotFoundError => "ContentNotFoundError",
            AuthenticationRequiredError => "AuthenticationRequiredError",
            ContentReSendError => "ContentReSendError",
            UnknownContentError => "UnknownContentError",
            ProtocolUnknownError => "ProtocolUnknownError",
            ProtocolInvalidOperationError => "ProtocolInvalidOperationError",
            ProtocolFailure => "ProtocolFailure",
        }
    }
}

/// `statusCodeFromHttp` (Qt 4.8): the network error an HTTP error status
/// becomes.
pub fn status_code_from_http(status: u16) -> NetworkError {
    use NetworkError::*;
    match status {
        401 => AuthenticationRequiredError,
        403 | 405 => ContentOperationNotPermittedError,
        404 => ContentNotFoundError,
        407 => ProxyAuthenticationRequiredError,
        418 => ProtocolInvalidOperationError,
        s if s > 500 => ProtocolUnknownError,
        s if s >= 400 => UnknownContentError,
        _ => ProtocolFailure,
    }
}

/// `QString` rendering of a URL in messages (`QUrl::toString()`):
/// percent-encoded octets of the path are shown decoded when they form
/// printable UTF-8, like Qt's "pretty" form.
pub fn qurl_to_string(url: &url::Url) -> String {
    let s = url.as_str();
    if !s.contains('%') || url.scheme() == "data" {
        return s.to_string();
    }
    let decoded: Vec<u8> = percent_decode(s.as_bytes());
    match String::from_utf8(decoded) {
        Ok(d) if !d.chars().any(|c| c.is_control()) => d,
        _ => s.to_string(),
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode `%XX` sequences, except the ones Qt keeps encoded in its pretty
/// form (`%25`, and delimiters that would change the URL structure).
fn percent_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' && i + 2 < s.len() {
            if let (Some(h), Some(l)) = (hex(s[i + 1]), hex(s[i + 2])) {
                let v = h * 16 + l;
                if !matches!(v, b'%' | b'#' | b'?' | b'/' | b'&' | b'=' | b'+') {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_statuses_map_like_qt48() {
        assert_eq!(status_code_from_http(404).code(), 203);
        assert_eq!(status_code_from_http(401).code(), 204);
        assert_eq!(status_code_from_http(403).code(), 202);
        assert_eq!(status_code_from_http(410).code(), 299);
        // 500 itself is "content" (> 500 only is protocol), verified on 0.12.6.
        assert_eq!(status_code_from_http(500).code(), 299);
        assert_eq!(status_code_from_http(502).code(), 301);
    }

    #[test]
    fn keys() {
        assert_eq!(NetworkError::from_code(3).unwrap().key(), "HostNotFoundError");
        assert_eq!(NetworkError::from_code(301).unwrap().key(), "ProtocolUnknownError");
    }

    #[test]
    fn display_decodes_spaces() {
        let u = url::Url::parse("file:///tmp/a%20b.css").unwrap();
        assert_eq!(qurl_to_string(&u), "file:///tmp/a b.css");
    }
}
