use std::io::{self, BufRead, BufReader, Cursor, Read};
use std::path::{Path, PathBuf};

use srr_server::http::{
    MAX_HEAD, Request, content_type, error_response, parse_request, read_head, resolve, response,
    websocket_handshake,
};

fn request(headers: &[(&str, &str)]) -> Request {
    Request {
        method: "GET".into(),
        target: "/ws".into(),
        headers: headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    }
}

#[test]
fn read_head_stops_at_the_empty_line() {
    let mut reader = Cursor::new(&b"GET / HTTP/1.1\r\nHost: x\r\n\r\nrest"[..]);
    assert_eq!(
        read_head(&mut reader).unwrap(),
        "GET / HTTP/1.1\r\nHost: x\r\n\r\n"
    );
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "rest");
}

#[test]
fn read_head_accepts_bare_newlines() {
    let mut reader = Cursor::new(&b"GET / HTTP/1.0\nHost: x\n\nbody"[..]);
    assert_eq!(
        read_head(&mut reader).unwrap(),
        "GET / HTTP/1.0\nHost: x\n\n"
    );
}

#[test]
fn read_head_keeps_the_rest_in_the_buffer() {
    let mut reader = BufReader::new(&b"GET /ws HTTP/1.1\r\n\r\n\x81\x80"[..]);
    read_head(&mut reader).unwrap();
    assert_eq!(reader.fill_buf().unwrap(), b"\x81\x80");
}

#[test]
fn read_head_fails_at_end_of_stream() {
    for input in [
        &b""[..],
        b"GET / HTTP/1.1\r\n",
        b"GET / HTTP/1.1\r\nHost: x",
    ] {
        let error = read_head(&mut Cursor::new(input)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof, "{input:?}");
    }
}

#[test]
fn read_head_refuses_huge_heads() {
    let mut head = b"GET / HTTP/1.1\r\n".to_vec();
    while head.len() <= MAX_HEAD {
        head.extend_from_slice(b"X-Filler: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
    }
    head.extend_from_slice(b"\r\n");
    let error = read_head(&mut Cursor::new(head)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "request head too large");

    // A single endless line too.
    let line = vec![b'a'; MAX_HEAD + 10];
    let error = read_head(&mut Cursor::new(line)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn read_head_refuses_invalid_utf8() {
    let error = read_head(&mut Cursor::new(&b"GET /\xFF HTTP/1.1\r\n\r\n"[..])).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn parses_a_request() {
    let request = parse_request(
        "GET /index.html?v=2 HTTP/1.1\r\nHost: example\r\nnot a header\r\nX-Spaced :  value  \r\n\r\n",
    )
    .unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.target, "/index.html?v=2");
    assert_eq!(request.path(), "/index.html");
    assert_eq!(
        request.headers,
        vec![
            ("Host".to_string(), "example".to_string()),
            ("X-Spaced".to_string(), "value".to_string()),
        ]
    );
    assert_eq!(request.header("host"), Some("example"));
    assert_eq!(request.header("x-spaced"), Some("value"));
    assert_eq!(request.header("missing"), None);
    assert_eq!(request.clone(), request);
}

#[test]
fn the_first_header_wins_and_values_keep_colons() {
    let request = parse_request("POST / HTTP/1.1\r\nHost: a:80\r\nhost: b\r\n\r\n").unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.header("HOST"), Some("a:80"));
}

#[test]
fn path_without_query() {
    let request = parse_request("GET /a/b HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(request.path(), "/a/b");
    let request = parse_request("GET /?x HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(request.path(), "/");
}

#[test]
fn rejects_malformed_request_lines() {
    for head in [
        "",
        "\r\n\r\n",
        "GET\r\n\r\n",
        "GET /\r\n\r\n",
        "GET / HTTP/1.1 extra\r\n\r\n",
        "GET / FTP/1.0\r\n\r\n",
    ] {
        assert_eq!(parse_request(head), None, "{head:?}");
    }
}

#[test]
fn websocket_upgrade_detection() {
    assert!(request(&[("Upgrade", "websocket"), ("Connection", "Upgrade")]).is_websocket_upgrade());
    assert!(
        request(&[
            ("upgrade", " WebSocket "),
            ("connection", "keep-alive, Upgrade")
        ])
        .is_websocket_upgrade()
    );
    assert!(!request(&[]).is_websocket_upgrade());
    assert!(!request(&[("Upgrade", "websocket")]).is_websocket_upgrade());
    assert!(!request(&[("Connection", "Upgrade")]).is_websocket_upgrade());
    assert!(!request(&[("Upgrade", "h2c"), ("Connection", "Upgrade")]).is_websocket_upgrade());
    assert!(
        !request(&[("Upgrade", "websocket"), ("Connection", "keep-alive")]).is_websocket_upgrade()
    );
}

#[test]
fn resolves_paths_under_the_root() {
    let root = Path::new("dist");
    assert_eq!(resolve(root, "/"), Some(root.join("index.html")));
    assert_eq!(resolve(root, "/srr.js"), Some(root.join("srr.js")));
    assert_eq!(
        resolve(root, "/assets/a.png"),
        Some(root.join("assets").join("a.png"))
    );
    assert_eq!(
        resolve(root, "/assets/"),
        Some(root.join("assets").join("index.html"))
    );
    assert_eq!(
        resolve(root, "//./assets//./b.svg"),
        Some(root.join("assets").join("b.svg"))
    );
}

#[test]
fn refuses_paths_leaving_the_root() {
    let root = Path::new("dist");
    for path in [
        "",
        "index.html",
        "/..",
        "/../secret",
        "/a/../../secret",
        "/a\\..\\secret",
        "/a\0b",
    ] {
        assert_eq!(resolve(root, path), None, "{path:?}");
    }
}

#[test]
fn content_types() {
    let cases = [
        ("index.html", "text/html; charset=utf-8"),
        ("srr.js", "text/javascript; charset=utf-8"),
        ("srr_bg.wasm", "application/wasm"),
        ("style.css", "text/css; charset=utf-8"),
        ("data.json", "application/json"),
        ("icon.png", "image/png"),
        ("logo.svg", "image/svg+xml"),
        ("archive.tar", "application/octet-stream"),
        ("README", "application/octet-stream"),
    ];
    for (file, expected) in cases {
        assert_eq!(content_type(&PathBuf::from(file)), expected, "{file}");
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_extensions_are_binary() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let path = Path::new(OsStr::from_bytes(b"file.\xFF"));
    assert_eq!(content_type(path), "application/octet-stream");
}

#[test]
fn responses() {
    assert_eq!(
        response("200 OK", "text/plain", b"hi"),
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\
          Cache-Control: no-store\r\nConnection: close\r\n\r\nhi"
    );
    assert_eq!(
        error_response("404 Not Found"),
        b"HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\n\
          Content-Length: 13\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n\
          404 Not Found"
    );
}

#[test]
fn websocket_handshake_answers_the_key() {
    assert_eq!(
        String::from_utf8(websocket_handshake(" dGhlIHNhbXBsZSBub25jZQ== ")).unwrap(),
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n"
    );
}
