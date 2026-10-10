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
//! [`MAX_HEAD`] and [`IO_TIMEOUT`]. Each connection is served on its own
//! thread, so a browser's idle pre-opened connection cannot hold up the real
//! one, and at most [`MAX_IN_FLIGHT`] are served at once (TUR-174): a
//! connection over the cap is closed unread, so a local process opening
//! thousands cannot start a thread for each. Anything that is not a `GET` of [`oauth::CALLBACK_PATH`] (a favicon,
//! a stray client) gets a 404, and a callback without this sign-in's `state`
//! ([`Loopback::expect_state`], compared in constant time by
//! [`oauth::carries_state`] before anything else in the URL is read) gets a
//! 400. Neither ends the listener: a stray tab or another local process
//! cannot end the sign-in. The first callback with the right `state` ends it,
//! and so do the timeout, a [`Canceller`] (the user's Cancel, TUR-174) and
//! dropping the [`Loopback`].

use std::io::{self, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ::calendar::oauth;

/// The most a request head may be: four times the old listener's buffer, so
/// a long Microsoft code and some cookies another local server left on
/// 127.0.0.1 fit, and still a small cap on what a local client can send.
pub const MAX_HEAD: usize = 16 * 1024;

/// How long one connection may take to send its head, or to take the reply.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// The most connections served at once. A browser opens a handful to one
/// host (the callback, a favicon, a spare it may never use), so this leaves
/// room for it and bounds what any other local process can tie up.
pub const MAX_IN_FLIGHT: usize = 16;

/// What wakes the sign-in waiting in [`Loopback::next_callback`].
enum Wake {
    /// The callback URL with the expected `state`.
    Callback(String),
    /// [`Canceller::cancel`].
    Cancelled,
}

/// Why [`Loopback::next_callback`] has no callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// No callback in time (or the listener stopped without one).
    TimedOut,
    /// [`Canceller::cancel`] was called.
    Cancelled,
}

/// A listener on a random 127.0.0.1 port. Dropping it stops it.
pub struct Loopback {
    /// Always 127.0.0.1, on a port the OS picked.
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    /// The `state` a callback must carry. Unset, no callback is ours.
    expected_state: Arc<OnceLock<String>>,
    /// The callback URL, `http://127.0.0.1:<port>/callback?…`, once: the
    /// first callback that carries the expected `state`. Or a cancel.
    callbacks: mpsc::Receiver<Wake>,
    /// For [`Self::canceller`].
    sender: mpsc::Sender<Wake>,
}

/// Ends a [`Loopback`]'s wait from another thread: the sign-in's Cancel.
#[derive(Clone)]
pub struct Canceller {
    port: u16,
    stop: Arc<AtomicBool>,
    sender: mpsc::Sender<Wake>,
}

impl Canceller {
    /// Wake [`Loopback::next_callback`] with [`Ended::Cancelled`] and stop
    /// listening. Nothing happens if the sign-in has already ended.
    pub fn cancel(&self) {
        // Gone only when the sign-in stopped waiting.
        let _ = self.sender.send(Wake::Cancelled);
        end(&self.stop, self.port);
    }
}

/// One of the [`MAX_IN_FLIGHT`] places; given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(in_flight: &Arc<AtomicUsize>) -> Option<Self> {
        in_flight
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                (now < MAX_IN_FLIGHT).then_some(now + 1)
            })
            .ok()
            .map(|_| Self(Arc::clone(in_flight)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Listen on a random loopback port, answering the callback with `page`.
pub fn listen(page: &'static str) -> io::Result<Loopback> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let addr = listener.local_addr()?;
    let port = addr.port();
    let stop = Arc::new(AtomicBool::new(false));
    let expected_state = Arc::new(OnceLock::new());
    let (sender, callbacks) = mpsc::channel();
    let stopped = Arc::clone(&stop);
    let expected = Arc::clone(&expected_state);
    let to_waiter = sender.clone();
    let in_flight = Arc::new(AtomicUsize::new(0));
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
                // Over the cap: closed unread, with no thread for it.
                let Some(slot) = Slot::take(&in_flight) else {
                    tracing::debug!("too many sign-in connections at once; closed one");
                    continue;
                };
                let listening = Listening {
                    port,
                    page,
                    expected_state: Arc::clone(&expected),
                    callbacks: to_waiter.clone(),
                    stop: Arc::clone(&stopped),
                };
                if let Err(error) = std::thread::Builder::new()
                    .name("meet-ai-sign-in-request".to_string())
                    .spawn(move || {
                        listening.serve(connection);
                        drop(slot);
                    })
                {
                    tracing::warn!(%error, "could not answer a sign-in request");
                }
            }
        })?;
    Ok(Loopback {
        addr,
        stop,
        expected_state,
        callbacks,
        sender,
    })
}

impl Loopback {
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Only a callback carrying `state` is ours. Set once, before the browser
    /// is opened; until then every callback is refused. A second call is
    /// ignored.
    pub fn expect_state(&self, state: &str) {
        let _ = self.expected_state.set(state.to_owned());
    }

    /// The first callback with the expected `state`, within `timeout`, or
    /// [`Ended::Cancelled`] once a [`Canceller`] says so.
    pub fn next_callback(&self, timeout: Duration) -> Result<String, Ended> {
        match self.callbacks.recv_timeout(timeout) {
            Ok(Wake::Callback(url)) => Ok(url),
            Ok(Wake::Cancelled) => Err(Ended::Cancelled),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                Err(Ended::TimedOut)
            }
        }
    }

    /// A handle that ends this wait from another thread.
    pub fn canceller(&self) -> Canceller {
        Canceller {
            port: self.addr.port(),
            stop: Arc::clone(&self.stop),
            sender: self.sender.clone(),
        }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        end(&self.stop, self.addr.port());
    }
}

/// Stop listening on `port`: set `stop` and wake the accept loop, which sees
/// it and closes the port. `false` when it had already ended.
fn end(stop: &AtomicBool, port: u16) -> bool {
    if stop.swap(true, Ordering::SeqCst) {
        return false;
    }
    // Failing means the accept loop is already gone.
    let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, port));
    true
}

/// Why a request was not the callback.
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    /// Not a readable HTTP/1 request, or it stopped before its blank line.
    Malformed,
    /// A head over [`MAX_HEAD`].
    TooLarge,
    /// A request for some other path, or not a `GET`, or a callback after
    /// the listener ended.
    NotFound,
    /// A callback without this sign-in's `state`.
    WrongState,
}

/// What one connection's thread needs from its listener.
struct Listening {
    port: u16,
    page: &'static str,
    expected_state: Arc<OnceLock<String>>,
    callbacks: mpsc::Sender<Wake>,
    stop: Arc<AtomicBool>,
}

impl Listening {
    fn serve(self, mut connection: TcpStream) {
        let _ = connection.set_read_timeout(Some(IO_TIMEOUT));
        let _ = connection.set_write_timeout(Some(IO_TIMEOUT));
        let port = self.port;
        let url = read_head(&mut connection)
            .and_then(|head| callback_target(&head))
            .map(|target| format!("http://127.0.0.1:{port}{target}"))
            // `state` first: a callback that is not ours leaves the listener
            // waiting for the one that is.
            .and_then(|url| {
                let ours = self
                    .expected_state
                    .get()
                    .is_some_and(|state| oauth::carries_state(&url, state));
                if ours {
                    Ok(url)
                } else {
                    Err(Refused::WrongState)
                }
            })
            // Only the first one counts; the listener has ended after it.
            .and_then(|url| {
                if end(&self.stop, port) {
                    Ok(url)
                } else {
                    Err(Refused::NotFound)
                }
            });
        let reply = match &url {
            Ok(_) => response("200 OK", self.page),
            Err(Refused::NotFound) => response("404 Not Found", ""),
            Err(Refused::TooLarge) => response("431 Request Header Fields Too Large", ""),
            Err(Refused::Malformed | Refused::WrongState) => response("400 Bad Request", ""),
        };
        // The browser may already be gone; the callback still counts.
        let _ = connection
            .write_all(reply.as_bytes())
            .and_then(|()| connection.flush());
        match url {
            Ok(url) => {
                // The receiver is gone only when the sign-in stopped waiting.
                let _ = self.callbacks.send(Wake::Callback(url));
            }
            Err(refused) => {
                tracing::debug!(?refused, "ignored a request to the sign-in listener");
            }
        }
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
