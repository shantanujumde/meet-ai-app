//! The sign-in's loopback listener (TUR-88): where the browser lands after
//! Google or Microsoft, at `http://127.0.0.1:<port>/callback?code=…&state=…`.
//!
//! This replaces `tauri-plugin-oauth`'s listener, which read each request
//! with one `read` into a 4048-byte buffer and only learned the URL from a
//! second request its page script sent (a `Full-Url` header, with the
//! browser's `Referer` repeating the URL). A Microsoft work account's code
//! is often over a kilobyte, so that request had little room to spare, and a
//! request that arrived in two reads was never seen at all: the sign-in hung
//! for five minutes and ended as cancelled.
//!
//! Here the URL is taken from the redirect's own request line, and the head
//! is read until its blank line, however many reads that takes, up to
//! [`MAX_HEAD`]. Each connection is served on its own thread, so a browser's
//! idle pre-opened connection cannot hold up the real one. Anything that is
//! not a `GET` of [`oauth::CALLBACK_PATH`] (a favicon, a stray client) gets an
//! error page and the listener keeps waiting. The `state` check that decides
//! whether a callback is ours is `CalendarAuth::finish_sign_in`'s, not this
//! file's.

use std::io::{self, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use ::calendar::oauth;

/// The most a request head may be. Far above any real redirect (a long code,
/// and cookies another local server left on 127.0.0.1), and still a cap.
pub const MAX_HEAD: usize = 256 * 1024;

/// How long one connection may take to send its head, or to take the reply.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// A listener on a random 127.0.0.1 port. Dropping it stops it.
pub struct Loopback {
    port: u16,
    stop: Arc<AtomicBool>,
    /// The callback URL, `http://127.0.0.1:<port>/callback?…`, each time the
    /// browser (or anything else) asks for the callback path.
    callbacks: mpsc::Receiver<String>,
}

/// Listen on a random loopback port, answering the callback with `page`.
pub fn listen(page: &'static str) -> io::Result<Loopback> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, callbacks) = mpsc::channel();
    let stopped = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("meet-ai-sign-in-listener".to_string())
        .spawn(move || {
            for connection in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(connection) = connection else {
                    continue;
                };
                let sender = sender.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("meet-ai-sign-in-request".to_string())
                    .spawn(move || serve(connection, port, page, &sender))
                {
                    tracing::warn!(%error, "could not answer a sign-in request");
                }
            }
        })?;
    Ok(Loopback {
        port,
        stop,
        callbacks,
    })
}

impl Loopback {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The first callback within `timeout`.
    pub fn next_callback(&self, timeout: Duration) -> Result<String, mpsc::RecvTimeoutError> {
        self.callbacks.recv_timeout(timeout)
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        if !self.stop.swap(true, Ordering::SeqCst) {
            // Wake the accept loop so it sees the flag and closes the port.
            // Failing means it is already gone.
            let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        }
    }
}

/// Why a request was not the callback.
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    /// Not a readable HTTP/1 request, or it stopped before its blank line.
    Malformed,
    /// A head over [`MAX_HEAD`].
    TooLarge,
    /// A request for some other path, or not a `GET`.
    NotFound,
}

fn serve(mut connection: TcpStream, port: u16, page: &str, callbacks: &mpsc::Sender<String>) {
    let _ = connection.set_read_timeout(Some(IO_TIMEOUT));
    let _ = connection.set_write_timeout(Some(IO_TIMEOUT));
    let target = read_head(&mut connection).and_then(|head| callback_target(&head));
    let reply = match &target {
        Ok(_) => response("200 OK", page),
        Err(Refused::NotFound) => response("404 Not Found", ""),
        Err(Refused::TooLarge) => response("431 Request Header Fields Too Large", ""),
        Err(Refused::Malformed) => response("400 Bad Request", ""),
    };
    // The browser may already be gone; the callback still counts.
    let _ = connection
        .write_all(reply.as_bytes())
        .and_then(|()| connection.flush());
    match target {
        Ok(target) => {
            // The receiver is gone only when the sign-in stopped waiting.
            let _ = callbacks.send(format!("http://127.0.0.1:{port}{target}"));
        }
        Err(refused) => tracing::debug!(?refused, "ignored a request to the sign-in listener"),
    }
}

/// A complete reply, closing the connection. `no-store` and `no-referrer`,
/// so the code in the URL is neither cached nor passed on.
fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The request head, up to and including its blank line, from as many reads
/// as it takes.
fn read_head(connection: &mut impl io::Read) -> Result<Vec<u8>, Refused> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = connection
            .read(&mut chunk)
            .map_err(|_| Refused::Malformed)?;
        if read == 0 {
            return Err(Refused::Malformed);
        }
        // Look for the end from just before the new bytes, in case the
        // blank line was split across reads.
        let from = head.len().saturating_sub(3);
        head.extend_from_slice(&chunk[..read]);
        if let Some(end) = find(&head[from..], b"\r\n\r\n") {
            head.truncate(from + end + 4);
            break;
        }
        if head.len() > MAX_HEAD {
            return Err(Refused::TooLarge);
        }
    }
    if head.len() > MAX_HEAD {
        return Err(Refused::TooLarge);
    }
    Ok(head)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The request target (`/callback?code=…`), when the head is a `GET` of the
/// callback path.
fn callback_target(head: &[u8]) -> Result<String, Refused> {
    let line_end = find(head, b"\r\n").ok_or(Refused::Malformed)?;
    let line = std::str::from_utf8(&head[..line_end]).map_err(|_| Refused::Malformed)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refused::Malformed);
    };
    if !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Err(Refused::Malformed);
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    if method != "GET" || path != oauth::CALLBACK_PATH {
        return Err(Refused::NotFound);
    }
    Ok(target.to_owned())
}

#[cfg(test)]
mod tests;
