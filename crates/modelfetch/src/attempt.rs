//! One download attempt: a request, a check that the answer is the bytes we
//! asked for, and the stream into the `.part` file.
//!
//! Each failure says whether another attempt could help ([`Failure::Retry`])
//! or not ([`Failure::Stop`]), so [`crate::retry`] does not make the user sit
//! through seconds of backoff before a certain error (TUR-159).

use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::StatusCode;
use stt::model::ModelSpec;
use tokio::io::AsyncWriteExt;

use crate::{CONNECT_TIMEOUT, Error, Progress, remove_if_present};

/// How often a wait (headers, a chunk, a backoff) looks at the cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// Why one attempt failed, and whether trying again could help.
#[derive(Debug)]
pub(crate) enum Failure {
    /// A dropped connection, a stall, a short body, a 5xx or a 429: the next
    /// attempt may well work, and resumes from the `.part` file.
    Retry(Error),
    /// A 404, a 403, a full disk, a cancel: the same request fails the same
    /// way, so it is reported now.
    Stop(Error),
}

/// Await `work`, or stop with [`Error::Cancelled`] once `cancel` is set.
///
/// The flag is polled rather than awaited so the caller can be a plain
/// `AtomicBool` that any thread sets.
pub(crate) async fn or_cancel<F: Future>(
    model: &'static str,
    cancel: &AtomicBool,
    work: F,
) -> Result<F::Output, Error> {
    let mut work = std::pin::pin!(work);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled(model));
        }
        tokio::select! {
            output = &mut work => return Ok(output),
            () = tokio::time::sleep(CANCEL_POLL) => {}
        }
    }
}

/// Whether an HTTP error status is worth another attempt: a server error or
/// "too many requests" may pass; anything else (404, 403, 416) will not.
pub(crate) fn retryable(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

/// The first byte of a `Content-Range: bytes <start>-<end>/<total>` value.
pub(crate) fn range_start(value: &str) -> Option<u64> {
    let range = value.trim().strip_prefix("bytes ")?;
    let (span, _total) = range.split_once('/')?;
    let (start, _end) = span.split_once('-')?;
    start.trim().parse().ok()
}

/// Stream the remaining bytes into `part_path`, returning the new total.
///
/// Fails with [`Failure::Retry`] if the server sends nothing for `stall`
/// while we wait for headers or a chunk.
pub(crate) async fn download(
    spec: &ModelSpec,
    part_path: &Path,
    resumed: u64,
    stall: Duration,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, Failure> {
    let stalled = || {
        Failure::Retry(Error::Download(format!(
            "{}: no data for {} ms; the connection stalled",
            spec.url,
            stall.as_millis()
        )))
    };
    let transport =
        |error: reqwest::Error| Failure::Retry(Error::Download(format!("{}: {error}", spec.url)));
    let disk = |error: std::io::Error| {
        Failure::Stop(Error::Download(format!("{}: {error}", part_path.display())))
    };

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| Failure::Stop(Error::Download(e.to_string())))?;

    let mut from = resumed;
    let response = loop {
        let mut request = client.get(spec.url);
        if from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={from}-"));
        }
        let response = or_cancel(spec.id, cancel, tokio::time::timeout(stall, request.send()))
            .await
            .map_err(Failure::Stop)?
            .map_err(|_| stalled())?
            .map_err(transport)?;

        let status = response.status();
        if !status.is_success() {
            let error = Error::Download(format!("{} returned {status}", spec.url));
            return Err(if retryable(status) {
                Failure::Retry(error)
            } else {
                Failure::Stop(error)
            });
        }
        if status != StatusCode::PARTIAL_CONTENT {
            break response;
        }
        // A 206 must be the range we asked for. Bytes from anywhere else,
        // appended, would fail the checksum and read as tampering, and that
        // throws the whole `.part` away. Start over instead, and say why.
        let start = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(range_start);
        if start == Some(from) {
            break response;
        }
        if from == 0 {
            return Err(Failure::Stop(Error::Download(format!(
                "{} answered with a range that does not start at the first byte",
                spec.url
            ))));
        }
        tracing::warn!(
            model = spec.id,
            asked = from,
            answered = ?start,
            "the server answered another range than asked for; downloading from the start"
        );
        remove_if_present(part_path).await;
        from = 0;
    };

    // A server that ignores `Range` answers 200 with the whole file. Appending
    // that to what we already have would corrupt it, so start the file over.
    let append = from > 0 && response.status() == StatusCode::PARTIAL_CONTENT;
    if from > 0 && !append {
        tracing::warn!(
            "the server ignored the resume request; downloading {} from the start",
            spec.id
        );
    }

    let mut written = if append { from } else { 0 };
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part_path)
        .await
        .map_err(disk)?;

    let mut stream = response.bytes_stream();
    // The bytes written so far stay in the `.part` file when this fails or
    // times out, so the retry resumes from them rather than starting over.
    let streamed: Result<(), Failure> = async {
        while let Some(chunk) =
            or_cancel(spec.id, cancel, tokio::time::timeout(stall, stream.next()))
                .await
                .map_err(Failure::Stop)?
                .map_err(|_| stalled())?
        {
            let chunk = chunk.map_err(transport)?;
            let after = written + chunk.len() as u64;
            if after > spec.bytes {
                return Err(Failure::Stop(Error::Download(format!(
                    "{} sent more than the {} bytes {} should be",
                    spec.url, spec.bytes, spec.id
                ))));
            }
            file.write_all(&chunk).await.map_err(disk)?;
            written = after;
            on_progress(Progress {
                downloaded_bytes: written,
                total_bytes: spec.bytes,
                verifying: false,
            });
        }
        Ok(())
    }
    .await;

    // Flush even when the stream failed. A `tokio::fs::File` write only
    // queues the bytes; dropping the handle without a flush lets them land
    // *after* the retry has measured or truncated the `.part` file, which
    // resumes from the wrong offset or corrupts it.
    let flushed = file.flush().await.map_err(disk);
    streamed?;
    flushed?;
    // The rename after verification is only atomic with respect to bytes that
    // actually reached the disk, so sync before the digest is computed.
    file.sync_all().await.map_err(disk)?;
    drop(file);

    if written != spec.bytes {
        // The body ended early: the connection closed before the end. The
        // next attempt resumes from here.
        return Err(Failure::Retry(Error::Download(format!(
            "{} is {written} bytes, expected {}",
            spec.id, spec.bytes
        ))));
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_gives_its_first_byte() {
        assert_eq!(range_start("bytes 5-15/16"), Some(5));
        assert_eq!(range_start(" bytes 0-0/1 "), Some(0));
        assert_eq!(range_start("bytes */16"), None);
        assert_eq!(range_start("items 5-15/16"), None);
        assert_eq!(range_start("bytes x-15/16"), None);
        assert_eq!(range_start(""), None);
    }

    #[test]
    fn only_server_errors_and_429_are_worth_another_attempt() {
        for status in [500, 502, 503, 504, 429] {
            assert!(retryable(StatusCode::from_u16(status).unwrap()), "{status}");
        }
        for status in [400, 401, 403, 404, 410, 416] {
            assert!(
                !retryable(StatusCode::from_u16(status).unwrap()),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn a_set_flag_stops_a_wait_that_would_never_end() {
        let cancel = AtomicBool::new(false);
        let waiting = or_cancel("test", &cancel, std::future::pending::<()>());
        cancel.store(true, Ordering::Relaxed);
        let error = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("the cancel was not noticed")
            .unwrap_err();
        assert!(matches!(error, Error::Cancelled("test")), "got {error:?}");
    }
}
