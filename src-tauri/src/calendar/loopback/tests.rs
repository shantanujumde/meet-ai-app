//! Real sockets on 127.0.0.1, sending what a browser sends. No browser.

use std::io::Read as _;
use std::net::TcpStream;
use std::time::Instant;

use super::*;

const PAGE: &str = "<!doctype html><p>Signed in.</p>";
const WAIT: Duration = Duration::from_secs(10);

/// A Microsoft work account's redirect as Chrome sends it: a code of about
/// 1.6 KB, `state`, `session_state`, and Chrome's usual headers, with the
/// cookies some other local dev server left on 127.0.0.1. About 4.4 KB, past
/// the old listener's one 4048-byte read.
fn chrome_redirect(port: u16) -> (String, String) {
    let code = format!("M.C540_BAY.2.U.{}", "a1B2c3D4e5".repeat(160));
    let target = format!(
        "/callback?code={code}&state=Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmE&session_state=0f1e2d3c-4b5a-6978-8091-a2b3c4d5e6f7"
    );
    let cookies = format!("Cookie: dev_session={}", "c".repeat(2000));
    let request = format!(
        "GET {target} HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Connection: keep-alive\r\n\
         sec-ch-ua: \"Google Chrome\";v=\"141\", \"Not?A_Brand\";v=\"8\", \"Chromium\";v=\"141\"\r\n\
         sec-ch-ua-mobile: ?0\r\n\
         sec-ch-ua-platform: \"macOS\"\r\n\
         Upgrade-Insecure-Requests: 1\r\n\
         User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36\r\n\
         Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8\r\n\
         Sec-Fetch-Site: cross-site\r\n\
         Sec-Fetch-Mode: navigate\r\n\
         Sec-Fetch-User: ?1\r\n\
         Sec-Fetch-Dest: document\r\n\
         Referer: https://login.microsoftonline.com/\r\n\
         Accept-Encoding: gzip, deflate, br, zstd\r\n\
         Accept-Language: en-GB,en-US;q=0.9,en;q=0.8\r\n\
         {cookies}\r\n\r\n"
    );
    (target, request)
}

/// Send `request` in `pieces`, a moment apart, and return the reply.
fn send(port: u16, request: &str, pieces: usize) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let bytes = request.as_bytes();
    let size = bytes.len().div_ceil(pieces.max(1));
    for piece in bytes.chunks(size) {
        stream.write_all(piece).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply
}

#[test]
fn a_realistic_microsoft_redirect_in_one_write_is_read_whole() {
    let listener = listen(PAGE).unwrap();
    let (target, request) = chrome_redirect(listener.port());
    assert!(request.len() > 4_048, "{} bytes", request.len());
    let reply = send(listener.port(), &request, 1);
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(reply.ends_with(PAGE));
    assert!(reply.contains("Cache-Control: no-store"));
    assert_eq!(
        listener.next_callback(WAIT).unwrap(),
        format!("http://127.0.0.1:{}{target}", listener.port())
    );
}

#[test]
fn a_redirect_that_arrives_in_pieces_is_still_read() {
    // A TCP read can return less than the whole request.
    let listener = listen(PAGE).unwrap();
    let (target, request) = chrome_redirect(listener.port());
    let reply = send(listener.port(), &request, 7);
    assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    let url = listener.next_callback(WAIT).unwrap();
    assert!(url.ends_with(&target));
}

#[test]
fn the_blank_line_split_across_two_reads_is_found() {
    let mut reader = io::Cursor::new(b"GET /callback?code=1 HTTP/1.1\r\nHost: x\r\n\r".to_vec())
        .chain(io::Cursor::new(b"\nleftover".to_vec()));
    let head = read_head(&mut reader).unwrap();
    assert!(head.ends_with(b"Host: x\r\n\r\n"));
    assert_eq!(
        callback_target(&head).unwrap(),
        "/callback?code=1".to_owned()
    );
}

#[test]
fn an_idle_connection_does_not_hold_up_the_browser() {
    // Browsers open spare connections that may never send a byte.
    let listener = listen(PAGE).unwrap();
    let _idle = TcpStream::connect((Ipv4Addr::LOCALHOST, listener.port())).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let reply = send(
        listener.port(),
        "GET /callback?state=s&code=c HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        1,
    );
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(listener.next_callback(WAIT).is_ok());
    assert!(started.elapsed() < IO_TIMEOUT, "{:?}", started.elapsed());
}

#[test]
fn other_requests_are_refused_and_the_listener_keeps_waiting() {
    let listener = listen(PAGE).unwrap();
    let port = listener.port();
    for (request, status) in [
        ("GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n", "404"),
        ("GET /callbackx?code=c HTTP/1.1\r\n\r\n", "404"),
        ("POST /callback?code=c HTTP/1.1\r\n\r\n", "404"),
        ("GET http://evil.example/callback HTTP/1.1\r\n\r\n", "400"),
        ("not http at all\r\n\r\n", "400"),
    ] {
        let reply = send(port, request, 1);
        assert!(
            reply.starts_with(&format!("HTTP/1.1 {status}")),
            "{request:?}: {reply}"
        );
    }
    let too_big = format!(
        "GET /callback?code=c HTTP/1.1\r\nCookie: {}\r\n\r\n",
        "x".repeat(MAX_HEAD + 10_000)
    );
    // The listener stops reading past the cap, so the end of this write
    // may be refused; only the reply matters.
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let _ = stream.write_all(too_big.as_bytes());
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    assert!(
        reply.starts_with(b"HTTP/1.1 431"),
        "{:?}",
        String::from_utf8_lossy(&reply)
    );
    assert!(
        listener.next_callback(Duration::from_millis(100)).is_err(),
        "a refused request is not a callback"
    );

    send(port, "GET /callback?state=s&code=c HTTP/1.1\r\n\r\n", 1);
    assert_eq!(
        listener.next_callback(WAIT).unwrap(),
        format!("http://127.0.0.1:{port}/callback?state=s&code=c")
    );
}

#[test]
fn a_dropped_listener_closes_its_port() {
    let listener = listen(PAGE).unwrap();
    let port = listener.port();
    drop(listener);
    let deadline = Instant::now() + WAIT;
    while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
        assert!(Instant::now() < deadline, "port {port} still open");
        std::thread::sleep(Duration::from_millis(20));
    }
}
