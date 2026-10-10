//! Lazy, resumable, checksum-verified download of the pinned whisper models.
//!
//! This is the only crate in the speech path that owns an HTTP stack. SPEC
//! L9/L10/L11 say transcription never touches the network; keeping `reqwest`
//! out of `crates/stt`'s dependency graph makes that structural rather than a
//! rule somebody has to remember. See `Cargo.toml` for the second reason (the
//! Windows seam guard).
//!
//! The catalogue itself — [`stt::model::ModelSpec`], the pinned URL and the
//! pinned SHA-256 — lives in `stt`, because `stt::registry` has to answer "is a
//! model on disk?" while offline. The dependency only points this way.
//!
//! ## What [`ensure`] guarantees
//!
//! * **Lazy.** If the finished file is already there, it returns immediately
//!   and makes no request. It does not re-hash a 574 MB file on every launch.
//! * **Resumable.** Bytes land in `<filename>.part` and a restart continues
//!   with an HTTP `Range` request instead of starting over.
//! * **Verified.** The `.part` file is renamed to its real name *only* after
//!   its SHA-256 matches the digest pinned in `stt::model`. A file under its
//!   real name is therefore always a file we verified, and the digest it was
//!   verified against is written beside it (`stt::model::digest_file`), so a
//!   catalogue entry whose digest later changes fetches the new file.
//! * **Pinned.** The URL comes from the catalogue, must be HTTPS, and names a
//!   commit rather than a branch. Nothing here takes a caller-supplied URL.
//! * **Cancellable.** The caller's flag is checked while waiting for the
//!   server, between chunks, during backoff and while hashing. A cancelled
//!   download keeps its `.part`, so the next one resumes.
//! * **Quiet.** Progress is reported when the percentage changes, or every
//!   100 ms, not per network chunk.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use stt::model::ModelSpec;

// One attempt, and whether its failure is worth another (TUR-159).
mod attempt;
mod retry;
// Fewer progress reports (TUR-159).
mod throttle;

// A model that is a folder of files, such as Parakeet (TUR-62).
mod folder;
pub use folder::ensure_folder;

use retry::RetryPolicy;

/// How long to wait for the server to answer at all.
///
/// There is no limit on the *whole* download. A slow but live download must
/// not be killed for being slow — on a bad hotel connection 574 MB
/// legitimately takes a long time. Silence is bounded separately, by
/// [`STALL_TIMEOUT`].
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the server may send nothing at all before the attempt fails.
///
/// Applies to the wait for the response headers and to every body chunk, not
/// to the download as a whole, so a slow connection that keeps trickling bytes
/// is never cut off. A connection that goes silent (Wi-Fi dropped, the TCP
/// session never reset) would otherwise leave the progress bar frozen
/// forever; failing it instead sends it through the retry, which resumes from
/// the `.part` file.
pub(crate) const STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Chunk size for the verification read. Big enough that hashing is I/O-bound.
const HASH_CHUNK: usize = 1 << 20;

/// Why a fetch failed.
///
/// Deliberately separate from [`stt::Error`]: nothing on the transcription
/// path can fail with a download error, and keeping the types apart is what
/// makes that true rather than merely intended.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The download did not finish, or came back the wrong size.
    #[error("model download failed: {0}")]
    Download(String),

    /// The bytes all arrived but they are not the file we pinned.
    ///
    /// Separate from [`Self::Download`] because this is a possible tampering
    /// signal rather than a flaky network, and the two need different words
    /// and a different button on screen.
    #[error("model {model} failed its checksum: expected {expected}, got {actual}")]
    Checksum {
        model: &'static str,
        expected: &'static str,
        actual: String,
    },

    /// The caller asked to stop. The `.part` file is kept for a resume.
    #[error("the download of {0} was cancelled")]
    Cancelled(&'static str),
}

impl Error {
    /// A stable tag for the failure, for callers that must branch on it.
    ///
    /// The UI's next step differs per kind — "try again" for a failed
    /// download, "download again from scratch" for a checksum mismatch — and
    /// matching on a message string is not a contract anyone can rely on.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Download(_) => ErrorKind::Download,
            Error::Checksum { .. } => ErrorKind::Checksum,
            Error::Cancelled(_) => ErrorKind::Cancelled,
        }
    }
}

/// The machine-readable half of [`Error`]. Pairs with the message, which is
/// already a whole sentence and is meant to be shown verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Download,
    Checksum,
    Cancelled,
}

/// How far along a download is.
///
/// Delivered to the `on_progress` callback. Exactly one of the two states:
/// bytes are arriving (`verifying == false`), or the bytes are all here and
/// the digest is being checked (`verifying == true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes on disk so far, including anything resumed from a previous run.
    pub downloaded_bytes: u64,
    /// The pinned size from the catalogue, so the UI can say "190 MB" honestly
    /// per model instead of hardcoding a number.
    pub total_bytes: u64,
    /// True once the bytes are all present and the SHA-256 is being computed.
    ///
    /// Hashing has no meaningful sub-progress to report, so a UI should switch
    /// to an indeterminate indicator when it sees this.
    pub verifying: bool,
}

impl Progress {
    /// Fraction downloaded, in `0.0..=1.0`. Returns `1.0` while verifying.
    pub fn fraction(&self) -> f64 {
        if self.verifying {
            return 1.0;
        }
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.downloaded_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }
}

/// Make sure `spec` is present and verified in `dir`, downloading if needed.
///
/// Returns the path to the finished file. Safe to call on every launch: the
/// already-installed case is one `metadata` call and no network traffic.
///
/// `on_progress` is called at least twice for any real download — once before
/// the first request goes out, and once when verification starts — so a UI
/// never has to decide whether a 0% bar means "connecting" or "frozen".
///
/// Setting `cancel` stops the download with [`Error::Cancelled`] within a
/// fraction of a second.
pub async fn ensure(
    spec: &ModelSpec,
    dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Error> {
    if !spec.url.starts_with("https://") && !dir.join(spec.filename).is_file() {
        return Err(Error::Download(format!(
            "{} is not pinned to an https URL",
            spec.id
        )));
    }
    ensure_with(spec, dir, &RetryPolicy::default(), cancel, on_progress).await
}

/// [`ensure`] with an injectable retry policy and no https check.
///
/// The https pin is enforced by the public [`ensure`]; this split lets the
/// tests point at a local plain-HTTP server with millisecond backoff.
async fn ensure_with(
    spec: &ModelSpec,
    dir: &Path,
    policy: &RetryPolicy,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Error> {
    let final_path = dir.join(spec.filename);
    if final_path.is_file() {
        // Present means verified: nothing reaches this name without passing
        // the digest check below. Against which digest is in the file beside
        // it.
        match stt::model::verified_digest(&final_path) {
            Some(digest) if digest == spec.sha256 => return Ok(final_path),
            // Verified by a build from before the digest file, against the
            // same catalogue: record it, once.
            None => {
                record_digest(spec, &final_path).await;
                return Ok(final_path);
            }
            // Verified against a digest the catalogue no longer pins: the
            // file upstream changed and so did the entry. Fetch the new one.
            Some(digest) => {
                tracing::warn!(
                    model = spec.id,
                    had = %digest,
                    pinned = spec.sha256,
                    "the installed model is not the pinned version; downloading it again"
                );
                remove_if_present(&final_path).await;
                remove_if_present(&stt::model::digest_file(&final_path)).await;
            }
        }
    }

    // Progress reports, throttled (TUR-159). The first one and the verifying
    // ones always get through.
    let mut throttle = throttle::Throttle::default();
    let on_progress = &mut |progress: Progress| {
        if throttle.admit(&progress, Instant::now()) {
            on_progress(progress);
        }
    };

    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", dir.display())))?;

    let part_path = dir.join(format!("{}.part", spec.filename));
    let mut resumed = part_size(&part_path).await;

    if resumed > spec.bytes {
        // Longer than the pinned size means the partial file is not this
        // model. Resuming from it would produce a checksum failure that looks
        // like tampering, so discard it instead and say why.
        tracing::warn!(
            path = %part_path.display(),
            resumed,
            expected = spec.bytes,
            "partial download is larger than the pinned size; starting over"
        );
        remove_if_present(&part_path).await;
        resumed = 0;
    }

    // Fire before the first request. Without this, a user on a slow network
    // watches a 0% bar with no explanation for up to CONNECT_TIMEOUT, and on
    // the already-complete resume path the download callback never fires at
    // all and the bar jumps straight to verifying.
    on_progress(Progress {
        downloaded_bytes: resumed,
        total_bytes: spec.bytes,
        verifying: false,
    });

    if resumed < spec.bytes {
        resumed = retry::download_with_retry(spec, &part_path, policy, cancel, on_progress).await?;
    }

    on_progress(Progress {
        downloaded_bytes: resumed,
        total_bytes: spec.bytes,
        verifying: true,
    });

    let actual = sha256_file(spec.id, &part_path, cancel).await?;
    if actual != spec.sha256 {
        // Delete the bad bytes. A checksum failure is a possible tampering
        // signal, and leaving the `.part` in place would make the UI's
        // "download again from scratch" button silently resume the same bad
        // file and fail identically.
        remove_if_present(&part_path).await;
        return Err(Error::Checksum {
            model: spec.id,
            expected: spec.sha256,
            actual,
        });
    }

    // The digest first: a crash between the two leaves a digest file with no
    // model, which reads as not installed, never a model with a wrong digest.
    record_digest(spec, &final_path).await;
    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", final_path.display())))?;

    tracing::info!(model = spec.id, path = %final_path.display(), "model verified");
    Ok(final_path)
}

/// Write the digest `final_path` is verified against beside it.
///
/// Not fatal when it fails: a model with no digest file counts as verified
/// against the catalogue it was downloaded with, which is still true.
async fn record_digest(spec: &ModelSpec, final_path: &Path) {
    let path = stt::model::digest_file(final_path);
    if let Err(error) = tokio::fs::write(&path, format!("{}\n", spec.sha256)).await {
        tracing::warn!(path = %path.display(), %error, "could not record the model's digest");
    }
}

/// SHA-256 of a file, lowercase hex. Stops between chunks once `cancel` is
/// set: hashing a gigabyte takes seconds.
async fn sha256_file(
    model: &'static str,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<String, Error> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", path.display())))?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|e| Error::Download(format!("{}: {e}", path.display())))?;
        if read == 0 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled(model));
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

pub(crate) async fn part_size(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0)
}

pub(crate) async fn remove_if_present(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), %error, "could not remove partial download");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny stand-in for a catalogue entry.
    ///
    /// The real models are 190 MB and 574 MB; writing and hashing one of those
    /// to test the resume bookkeeping would make the suite slow for no extra
    /// coverage. The URL points nowhere on purpose — every test here asserts
    /// on a path that must not make a request.
    const TINY: ModelSpec = ModelSpec {
        id: "test-tiny",
        filename: "ggml-test-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/does-not-exist.bin",
        // SHA-256 of eight zero bytes, which is what the tests write.
        sha256: "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc",
        bytes: 8,
        facts: stt::model::ModelFacts::NONE,
    };

    /// A cancel flag nobody sets.
    pub(crate) static NEVER: AtomicBool = AtomicBool::new(false);

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn fraction_is_clamped_and_reads_one_while_verifying() {
        let progress = Progress {
            downloaded_bytes: 50,
            total_bytes: 200,
            verifying: false,
        };
        assert_eq!(progress.fraction(), 0.25);

        let verifying = Progress {
            downloaded_bytes: 0,
            total_bytes: 200,
            verifying: true,
        };
        assert_eq!(verifying.fraction(), 1.0);

        let unknown_size = Progress {
            downloaded_bytes: 10,
            total_bytes: 0,
            verifying: false,
        };
        assert_eq!(unknown_size.fraction(), 0.0);
    }

    #[test]
    fn the_catalogue_digests_are_what_this_crate_compares_against() {
        // Guards the seam rather than the download: if `stt::model` ever stops
        // exposing a pinned digest per model, verification here is meaningless.
        for spec in stt::model::MODELS {
            assert_eq!(spec.sha256.len(), 64, "{} has no pinned digest", spec.id);
            assert!(spec.url.starts_with("https://"), "{} is not https", spec.id);
        }
    }

    #[tokio::test]
    async fn an_installed_model_is_returned_without_touching_the_network() {
        // Nothing is listening and the URL does not resolve, so a request
        // would fail the test rather than pass it. That is the point.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(dir.join(TINY.filename), b"already verified").unwrap();

        let mut calls = 0;
        let path = ensure(&TINY, &dir, &NEVER, &mut |_| calls += 1)
            .await
            .unwrap();

        assert_eq!(path, dir.join(TINY.filename));
        assert_eq!(calls, 0, "the installed path must not report progress");
        // Installed before digest files: the pinned digest is recorded.
        assert_eq!(
            stt::model::verified_digest(&path).as_deref(),
            Some(TINY.sha256)
        );
    }

    #[tokio::test]
    async fn a_promoted_part_records_the_digest_it_was_verified_against() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(dir.join(format!("{}.part", TINY.filename)), [0u8; 8]).unwrap();
        let path = ensure(&TINY, &dir, &NEVER, &mut |_| {}).await.unwrap();
        assert_eq!(
            stt::model::verified_digest(&path).as_deref(),
            Some(TINY.sha256)
        );
        assert!(stt::model::is_installed(&TINY, &dir));
    }

    #[tokio::test]
    async fn a_model_verified_against_another_digest_is_fetched_again() {
        // The catalogue entry changed since this file was verified. Nothing
        // listens at the URL, so the re-fetch fails, but the stale file must
        // be gone rather than returned as if it were the pinned one.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let file = dir.join(TINY.filename);
        std::fs::write(&file, b"old weights").unwrap();
        std::fs::write(stt::model::digest_file(&file), "0".repeat(64)).unwrap();
        let dead = ModelSpec {
            url: "http://127.0.0.1:1/x",
            ..TINY
        };
        let once = RetryPolicy {
            max_retries: 0,
            base_delay: Duration::ZERO,
            stall_timeout: STALL_TIMEOUT,
        };
        let result = ensure_with(&dead, &dir, &once, &NEVER, &mut |_| {}).await;
        assert!(matches!(result, Err(Error::Download(_))), "got {result:?}");
        assert!(!file.exists(), "the stale model was kept");
        assert!(!stt::model::digest_file(&file).exists());
    }

    #[tokio::test]
    async fn a_cancel_while_hashing_keeps_the_part_for_a_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let part = dir.join(format!("{}.part", TINY.filename));
        std::fs::write(&part, [0u8; 8]).unwrap();
        let cancel = AtomicBool::new(true);
        let error = ensure(&TINY, &dir, &cancel, &mut |_| {}).await.unwrap_err();
        assert!(matches!(error, Error::Cancelled(_)), "got {error:?}");
        assert_eq!(error.kind(), ErrorKind::Cancelled);
        assert!(part.exists());
        assert!(!dir.join(TINY.filename).exists());
    }

    #[tokio::test]
    async fn a_complete_part_file_is_verified_and_promoted_without_downloading() {
        // The resume case where the previous run got all the bytes. No request
        // goes out, and the caller still gets a progress callback before the
        // verifying one — otherwise the bar jumps from nothing to "checking".
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(dir.join(format!("{}.part", TINY.filename)), [0u8; 8]).unwrap();

        let mut seen: Vec<Progress> = Vec::new();
        let path = ensure(&TINY, &dir, &NEVER, &mut |progress| seen.push(progress))
            .await
            .unwrap();

        assert_eq!(path, dir.join(TINY.filename));
        assert_eq!(seen.len(), 2, "expected a start and a verifying callback");
        assert_eq!(seen[0].downloaded_bytes, TINY.bytes);
        assert!(!seen[0].verifying);
        assert!(seen[1].verifying);
        assert!(!dir.join(format!("{}.part", TINY.filename)).exists());
    }

    #[tokio::test]
    async fn a_wrong_digest_is_a_checksum_error_and_deletes_the_bad_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let part = dir.join(format!("{}.part", TINY.filename));
        std::fs::write(&part, b"notzeros").unwrap();

        let error = ensure(&TINY, &dir, &NEVER, &mut |_| {}).await.unwrap_err();

        // Must be ModelChecksum, not ModelDownload: the UI shows a
        // security-flavoured screen for one and "try again" for the other.
        assert!(matches!(error, Error::Checksum { .. }), "got {error:?}");
        assert!(
            !dir.join(TINY.filename).exists(),
            "an unverified file was promoted"
        );
        assert!(
            !part.exists(),
            "\"download again from scratch\" would resume bad bytes"
        );
    }

    #[tokio::test]
    async fn an_oversized_part_file_is_discarded_rather_than_resumed() {
        // Resuming past the pinned size would surface as a checksum failure,
        // which reads as tampering. Start over instead.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let part = dir.join(format!("{}.part", TINY.filename));
        std::fs::write(&part, [0u8; 32]).unwrap();

        let mut first: Option<Progress> = None;
        // The download then fails for lack of a network, which is fine — the
        // assertion is about what was reported before the request went out.
        let dead = ModelSpec {
            url: "http://127.0.0.1:1/x",
            ..TINY
        };
        let fast = RetryPolicy {
            max_retries: 0,
            base_delay: Duration::ZERO,
            stall_timeout: STALL_TIMEOUT,
        };
        let _ = ensure_with(&dead, &dir, &fast, &NEVER, &mut |progress| {
            first.get_or_insert(progress);
        })
        .await;

        assert_eq!(
            first
                .expect("a progress callback fires before the first request")
                .downloaded_bytes,
            0,
            "an oversized partial must not be resumed"
        );
    }
}
