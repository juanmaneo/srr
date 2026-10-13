//! Just enough HTTP/1.1 to serve the web build and accept WebSocket upgrades.
//!
//! Every connection carries one request and is closed after the response (or, for a WebSocket
//! upgrade, after the WebSocket session).

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use crate::websocket::accept_key;

/// Requests with a longer head are refused.
pub const MAX_HEAD: usize = 16 * 1024;

/// The request line and headers of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The method, e.g. `GET`.
    pub method: String,
    /// The request target as sent, e.g. `/index.html?v=2`.
    pub target: String,
    /// Header names and values, in order; names keep their original case.
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// The value of the first header called `name`, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The target without its query string.
    pub fn path(&self) -> &str {
        self.target
            .split_once('?')
            .map_or(self.target.as_str(), |(path, _)| path)
    }

    /// Whether this is a WebSocket opening handshake: `Upgrade: websocket` and a
    /// `Connection` header listing `upgrade`.
    pub fn is_websocket_upgrade(&self) -> bool {
        let upgrade = self
            .header("upgrade")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("websocket"));
        let connection = self.header("connection").is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
        upgrade && connection
    }
}

/// Reads a request head: everything up to and including the first empty line.
///
/// Reads line by line from a buffered reader, so bytes after the head stay in the buffer for
/// whoever reads next (the WebSocket frames, for an upgrade).
///
/// # Errors
///
/// I/O errors of `reader`, `UnexpectedEof` when the stream ends before the empty line, and
/// `InvalidData` when the head exceeds [`MAX_HEAD`] or is not UTF-8.
pub fn read_head(reader: &mut impl BufRead) -> io::Result<String> {
    let mut head = String::new();
    loop {
        let read = reader.read_line(&mut head)?;
        if read == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if head.len() > MAX_HEAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too large",
            ));
        }
        if head.ends_with("\r\n\r\n") || head.ends_with("\n\n") {
            return Ok(head);
        }
    }
}

/// Parses a request head; `None` when the request line is malformed.
///
/// Header lines without a colon are skipped.
///
/// ```
/// use srr_server::http::parse_request;
///
/// let request = parse_request("GET /ws?x=1 HTTP/1.1\r\nUpgrade: websocket\r\n\r\n").unwrap();
/// assert_eq!(request.method, "GET");
/// assert_eq!(request.path(), "/ws");
/// assert_eq!(request.header("UPGRADE"), Some("websocket"));
/// ```
pub fn parse_request(head: &str) -> Option<Request> {
    let mut lines = head.lines();
    let mut parts = lines.next()?.split_whitespace();
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if !version.starts_with("HTTP/") {
        return None;
    }
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .collect();
    Some(Request {
        method: method.to_string(),
        target: target.to_string(),
        headers,
    })
}

/// The file under `root` that serves `path`; `/` and directories mean their `index.html`.
///
/// Returns `None` for paths that do not start with `/` or try to leave `root` (`..`
/// segments, backslashes, NUL bytes).
///
/// ```
/// use std::path::Path;
/// use srr_server::http::resolve;
///
/// let root = Path::new("dist");
/// assert_eq!(resolve(root, "/"), Some(root.join("index.html")));
/// assert_eq!(resolve(root, "/srr.js"), Some(root.join("srr.js")));
/// assert_eq!(resolve(root, "/../secret"), None);
/// ```
pub fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = path.strip_prefix('/')?;
    if relative.contains(['\\', '\0']) {
        return None;
    }
    let mut resolved = root.to_path_buf();
    for segment in relative.split('/') {
        match segment {
            ".." => return None,
            "" | "." => {}
            name => resolved.push(name),
        }
    }
    if relative.is_empty() || relative.ends_with('/') {
        resolved.push("index.html");
    }
    Some(resolved)
}

/// The `Content-Type` of a served file, from its extension.
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        // Required by `WebAssembly.instantiateStreaming`.
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// A complete response that closes the connection. Nothing is cached, so a new build is
/// picked up by a simple reload.
pub fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A plain-text error response.
pub fn error_response(status: &str) -> Vec<u8> {
    response(status, "text/plain; charset=utf-8", status.as_bytes())
}

/// The `101 Switching Protocols` answer to a WebSocket handshake carrying `key`.
pub fn websocket_handshake(key: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key.trim())
    )
    .into_bytes()
}
