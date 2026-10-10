//! Retry with backoff around a single download attempt.
//!
//! A failed attempt leaves its bytes in the `.part` file, so each retry
//! re-measures the file and `download` resumes with a `Range` request (or
//! restarts from zero if the server ignores ranges).
//!
//! Only a failure another attempt could fix is retried: a dropped connection,
//! a stall, a short body, a 5xx or a 429 (TUR-159). A 404, a 403, a 416 or a
//! full disk is reported at once rather than after seconds of backoff.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use stt::model::ModelSpec;

use crate::attempt::{Failure, download, or_cancel};
use crate::{Error, Progress, STALL_TIMEOUT, part_size, remove_if_present};

/// How many times to retry after the first failure.
const MAX_RETRIES: u32 = 3;

/// First backoff delay; doubles each retry (1s, 2s, 4s).
const BASE_DELAY: Duration = Duration::from_secs(1);

/// When an attempt is given up, and how retries are spaced. Injectable so
/// tests do not sleep for seconds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    /// Silence after which an attempt fails and is retried; see
    /// [`STALL_TIMEOUT`].
    pub stall_timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            base_delay: BASE_DELAY,
            stall_timeout: STALL_TIMEOUT,
        }
    }
}

impl RetryPolicy {
    /// Delay before retry number `retry` (0-based): base, 2x, 4x...
    fn delay(&self, retry: u32) -> Duration {
        self.base_delay.saturating_mul(1u32 << retry.min(16))
    }
}

/// Run [`download`] until it succeeds, fails in a way no retry fixes, is
/// cancelled, or the retries are used up.
pub(crate) async fn download_with_retry(
    spec: &ModelSpec,
    part_path: &Path,
    policy: &RetryPolicy,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, Error> {
    let mut retry = 0;
    loop {
        let mut have = part_size(part_path).await;
        if have > spec.bytes {
            remove_if_present(part_path).await;
            have = 0;
        }
        if have == spec.bytes {
            // A previous attempt got every byte before failing; verification
            // decides whether they are good.
            return Ok(have);
        }

        let attempt = download(
            spec,
            part_path,
            have,
            policy.stall_timeout,
            cancel,
            on_progress,
        );
        match attempt.await {
            Ok(total) => return Ok(total),
            Err(Failure::Retry(error)) if retry < policy.max_retries => {
                let delay = policy.delay(retry);
                tracing::warn!(%error, retry = retry + 1, ?delay, "download failed; retrying");
                or_cancel(spec.id, cancel, tokio::time::sleep(delay)).await?;
                retry += 1;
            }
            Err(Failure::Retry(error) | Failure::Stop(error)) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::tests::NEVER;
    use crate::{ensure_with, hex};

    const FAST: RetryPolicy = RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(1),
        stall_timeout: STALL_TIMEOUT,
    };

    #[derive(Clone, Copy)]
    struct Script {
        /// Connections that send only `partial` bytes and then hang up.
        fail_first: usize,
        partial: usize,
        ranges: bool,
        /// Failing connections go silent after `partial` bytes, keeping the
        /// socket open, instead of hanging up. This is a dropped Wi-Fi link:
        /// no FIN, no RST, just nothing.
        stall: bool,
        /// Gap between single-byte writes on a full send. Zero sends the body
        /// in one write.
        trickle: Duration,
        /// Answer every request with this status and no body.
        status: Option<u16>,
        /// Answer a range request with a 206 that starts this many bytes
        /// before the byte asked for.
        misalign: usize,
    }

    const PLAIN: Script = Script {
        fail_first: 0,
        partial: 0,
        ranges: true,
        stall: false,
        trickle: Duration::ZERO,
        status: None,
        misalign: 0,
    };

    struct Server {
        url: String,
        /// The `Range` header of every request, in order.
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }

    fn body() -> Vec<u8> {
        (0u8..16).collect()
    }

    fn serve(body: Vec<u8>, script: Script) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.bin", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            // Stalled connections are parked here so they stay open.
            let mut parked = Vec::new();
            for (index, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { return };
                let mut request = Vec::new();
                let mut buf = [0u8; 512];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request).to_string();
                let range = text
                    .lines()
                    .find_map(|l| l.strip_prefix("range: ").or(l.strip_prefix("Range: ")))
                    .map(str::to_string);
                log.lock().unwrap().push(range.clone());
                if let Some(status) = script.status {
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\n\
                             Connection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    );
                    continue;
                }

                let start = range
                    .as_deref()
                    .filter(|_| script.ranges)
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.strip_suffix('-'))
                    .and_then(|r| r.parse::<usize>().ok())
                    .map(|s| s.saturating_sub(script.misalign));
                let tail = &body[start.unwrap_or(0)..];
                let head = match start {
                    Some(s) => format!(
                        "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\n\
                         Content-Range: bytes {s}-{}/{}\r\nContent-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        body.len() - 1,
                        body.len(),
                        tail.len()
                    ),
                    None => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        tail.len()
                    ),
                };
                let failing = index < script.fail_first;
                let send = if failing {
                    &tail[..script.partial.min(tail.len())]
                } else {
                    tail
                };
                // No Nagle batching, so trickled bytes leave one at a time.
                let _ = stream.set_nodelay(true);
                let _ = stream.write_all(head.as_bytes());
                if !failing && !script.trickle.is_zero() {
                    for byte in send {
                        let _ = stream.write_all(std::slice::from_ref(byte));
                        let _ = stream.flush();
                        std::thread::sleep(script.trickle);
                    }
                } else {
                    let _ = stream.write_all(send);
                }
                let _ = stream.flush();
                if failing && script.stall {
                    parked.push(stream);
                }
            }
        });
        Server { url, seen }
    }

    fn spec(url: &str, bytes: &[u8], digest_of: &[u8]) -> ModelSpec {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        ModelSpec {
            id: "test-retry",
            filename: "retry.bin",
            url: leak(url.to_string()),
            sha256: leak(hex(&Sha256::digest(digest_of))),
            bytes: bytes.len() as u64,
            facts: stt::model::ModelFacts::NONE,
        }
    }

    #[tokio::test]
    async fn fails_twice_then_succeeds() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 2,
                partial: 3,
                ranges: true,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let path = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(server.seen.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn resume_sends_range_and_bytes_match() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 1,
                partial: 5,
                ranges: true,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let path = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        let seen = server.seen.lock().unwrap().clone();
        assert_eq!(seen, vec![None, Some("bytes=5-".to_string())]);
    }

    #[tokio::test]
    async fn server_without_ranges_restarts_cleanly() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 1,
                partial: 5,
                ranges: false,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let path = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(server.seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn gives_up_after_three_retries() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 100,
                partial: 2,
                ranges: true,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let error = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Download(_)), "got {error:?}");
        assert_eq!(server.seen.lock().unwrap().len(), 4, "1 try + 3 retries");
        assert!(!dir.join("retry.bin").exists());
    }

    #[tokio::test]
    async fn checksum_mismatch_still_errors_without_retrying() {
        let data = body();
        let server = serve(data.clone(), PLAIN);
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, b"some other bytes");
        let error = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Checksum { .. }), "got {error:?}");
        assert_eq!(server.seen.lock().unwrap().len(), 1);
        assert!(!dir.join("retry.bin").exists());
        assert!(!dir.join("retry.bin.part").exists());
    }

    /// Fails the test instead of hanging it, which is the bug being tested.
    async fn within<T>(limit: Duration, work: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(limit, work)
            .await
            .expect("the download hung instead of failing and retrying")
    }

    #[tokio::test]
    async fn a_connection_that_goes_silent_mid_body_is_retried_and_resumed() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 1,
                partial: 5,
                stall: true,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let policy = RetryPolicy {
            stall_timeout: Duration::from_millis(200),
            ..FAST
        };
        let path = within(
            Duration::from_secs(10),
            ensure_with(&s, &dir, &policy, &NEVER, &mut |_| {}),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        // The retry resumed from the five bytes the stalled attempt kept.
        let seen = server.seen.lock().unwrap().clone();
        assert_eq!(seen, vec![None, Some("bytes=5-".to_string())]);
    }

    #[tokio::test]
    async fn a_server_that_always_stalls_ends_in_a_download_error() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 100,
                partial: 2,
                stall: true,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let policy = RetryPolicy {
            stall_timeout: Duration::from_millis(200),
            ..FAST
        };
        let error = within(
            Duration::from_secs(10),
            ensure_with(&s, &dir, &policy, &NEVER, &mut |_| {}),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Download(_)), "got {error:?}");
        assert!(error.to_string().contains("stalled"), "got {error}");
        assert_eq!(server.seen.lock().unwrap().len(), 4, "1 try + 3 retries");
    }

    #[tokio::test]
    async fn a_slow_but_live_download_is_not_cut_off() {
        // 16 bytes, 50 ms apart: the whole body takes ~800 ms, well past the
        // 300 ms stall limit, but no single gap comes near it. The limit is
        // on silence, not on total time.
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                trickle: Duration::from_millis(50),
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let policy = RetryPolicy {
            max_retries: 0,
            stall_timeout: Duration::from_millis(300),
            ..FAST
        };
        let path = within(
            Duration::from_secs(10),
            ensure_with(&s, &dir, &policy, &NEVER, &mut |_| {}),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        assert_eq!(server.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_resume_answered_with_another_range_starts_over_instead_of_corrupting() {
        // The first attempt keeps 5 bytes; the resume asks for `bytes=5-` and
        // the server answers a 206 that starts at byte 2. Appending that would
        // fail the checksum and throw the `.part` away as tampering.
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                fail_first: 1,
                partial: 5,
                misalign: 3,
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let path = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);
        let seen = server.seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![None, Some("bytes=5-".to_string()), None],
            "the wrong range is dropped and the file fetched from the start"
        );
    }

    #[tokio::test]
    async fn a_404_is_reported_at_once_without_retrying() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                status: Some(404),
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        // Seconds of backoff per retry: a retry would blow the time limit.
        let slow = RetryPolicy {
            base_delay: Duration::from_secs(5),
            ..FAST
        };
        let error = within(
            Duration::from_secs(4),
            ensure_with(&s, &dir, &slow, &NEVER, &mut |_| {}),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Download(_)), "got {error:?}");
        assert!(error.to_string().contains("404"), "got {error}");
        assert_eq!(server.seen.lock().unwrap().len(), 1, "no retry");
    }

    #[tokio::test]
    async fn a_503_is_retried() {
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                status: Some(503),
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let error = ensure_with(&s, &dir, &FAST, &NEVER, &mut |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("503"), "got {error}");
        assert_eq!(server.seen.lock().unwrap().len(), 4, "1 try + 3 retries");
    }

    #[tokio::test]
    async fn a_cancel_mid_download_stops_it_and_keeps_the_part() {
        // 16 bytes, 100 ms apart: the cancel lands well before the end.
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                trickle: Duration::from_millis(100),
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let cancel = AtomicBool::new(false);
        let error = within(
            Duration::from_secs(5),
            ensure_with(&s, &dir, &FAST, &cancel, &mut |progress| {
                if progress.downloaded_bytes >= 2 {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, Error::Cancelled("test-retry")),
            "got {error:?}"
        );
        assert_eq!(
            server.seen.lock().unwrap().len(),
            1,
            "a cancel is not retried"
        );
        let kept = std::fs::metadata(dir.join("retry.bin.part")).unwrap().len();
        assert!((2..16).contains(&kept), "kept {kept} bytes");
        assert!(!dir.join("retry.bin").exists());
    }

    #[tokio::test]
    async fn progress_is_reported_per_percentage_not_per_chunk() {
        // 16 one-byte chunks are 16 different percentages, so all may pass;
        // what must not happen is a report with no change and no tick.
        let data = body();
        let server = serve(
            data.clone(),
            Script {
                trickle: Duration::from_millis(1),
                ..PLAIN
            },
        );
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let s = spec(&server.url, &data, &data);
        let mut seen = Vec::new();
        ensure_with(&s, &dir, &FAST, &NEVER, &mut |progress| seen.push(progress))
            .await
            .unwrap();
        let downloading: Vec<_> = seen.iter().filter(|p| !p.verifying).collect();
        let mut percents: Vec<_> = downloading
            .iter()
            .map(|p| (p.fraction() * 100.0) as u64)
            .collect();
        percents.dedup();
        assert_eq!(
            percents.len(),
            downloading.len(),
            "a repeated percentage: {seen:?}"
        );
        assert!(seen.last().unwrap().verifying);
    }

    #[test]
    fn default_stall_timeout_is_30_seconds() {
        assert_eq!(
            RetryPolicy::default().stall_timeout,
            Duration::from_secs(30)
        );
    }

    #[test]
    fn default_backoff_is_1_2_4_seconds() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
        assert_eq!(
            [p.delay(0), p.delay(1), p.delay(2)],
            [
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ]
        );
    }
}
