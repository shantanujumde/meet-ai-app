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
//!   real name is therefore always a file we verified.
//! * **Pinned.** The URL comes from the catalogue and must be HTTPS. Nothing
//!   here takes a caller-supplied URL.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use stt::model::ModelSpec;
use tokio::io::AsyncWriteExt;

/// How long to wait for the server to answer at all.
///
/// Only the *connect* phase is bounded. A slow but live download must not be
/// killed for being slow — on a bad hotel connection 574 MB legitimately takes
/// a long time, and cancelling it would throw away resumable progress.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

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
        }
    }
}

/// The machine-readable half of [`Error`]. Pairs with the message, which is
/// already a whole sentence and is meant to be shown verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Download,
    Checksum,
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
pub async fn ensure(
    spec: &ModelSpec,
    dir: &Path,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Error> {
    let final_path = dir.join(spec.filename);
    if final_path.is_file() {
        // Present means verified: nothing reaches this name without passing
        // the digest check below.
        return Ok(final_path);
    }

    if !spec.url.starts_with("https://") {
        return Err(Error::Download(format!(
            "{} is not pinned to an https URL",
            spec.id
        )));
    }

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
        resumed = download(spec, &part_path, resumed, on_progress).await?;
    }

    on_progress(Progress {
        downloaded_bytes: resumed,
        total_bytes: spec.bytes,
        verifying: true,
    });

    let actual = sha256_file(&part_path).await?;
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

    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", final_path.display())))?;

    tracing::info!(model = spec.id, path = %final_path.display(), "model verified");
    Ok(final_path)
}

/// Stream the remaining bytes into `part_path`, returning the new total.
async fn download(
    spec: &ModelSpec,
    part_path: &Path,
    resumed: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<u64, Error> {
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| Error::Download(e.to_string()))?;

    let mut request = client.get(spec.url);
    if resumed > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resumed}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", spec.url)))?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::Download(format!(
            "{} returned {status}",
            spec.url
        )));
    }

    // A server that ignores `Range` answers 200 with the whole file. Appending
    // that to what we already have would corrupt it, so start the file over.
    let append = resumed > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if resumed > 0 && !append {
        tracing::warn!(
            "the server ignored the resume request; downloading {} from the start",
            spec.id
        );
    }

    let mut written = if append { resumed } else { 0 };
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(part_path)
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", part_path.display())))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| Error::Download(format!("{}: {e}", spec.url)))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| Error::Download(format!("{}: {e}", part_path.display())))?;
        written += chunk.len() as u64;
        on_progress(Progress {
            downloaded_bytes: written,
            total_bytes: spec.bytes,
            verifying: false,
        });
    }

    file.flush()
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", part_path.display())))?;
    // The rename below is only atomic with respect to bytes that actually
    // reached the disk, so sync before the digest is computed and trusted.
    file.sync_all()
        .await
        .map_err(|e| Error::Download(format!("{}: {e}", part_path.display())))?;
    drop(file);

    if written != spec.bytes {
        return Err(Error::Download(format!(
            "{} is {written} bytes, expected {}",
            spec.id, spec.bytes
        )));
    }

    Ok(written)
}

/// SHA-256 of a file, lowercase hex.
async fn sha256_file(path: &Path) -> Result<String, Error> {
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
        hasher.update(&buffer[..read]);
    }

    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

async fn part_size(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0)
}

async fn remove_if_present(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), %error, "could not remove partial download");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meet-ai-modelfetch-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
    };

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
        let dir = temp_dir("installed");
        std::fs::write(dir.join(TINY.filename), b"already verified").unwrap();

        let mut calls = 0;
        let path = ensure(&TINY, &dir, &mut |_| calls += 1).await.unwrap();

        assert_eq!(path, dir.join(TINY.filename));
        assert_eq!(calls, 0, "the installed path must not report progress");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_complete_part_file_is_verified_and_promoted_without_downloading() {
        // The resume case where the previous run got all the bytes. No request
        // goes out, and the caller still gets a progress callback before the
        // verifying one — otherwise the bar jumps from nothing to "checking".
        let dir = temp_dir("complete-part");
        std::fs::write(dir.join(format!("{}.part", TINY.filename)), [0u8; 8]).unwrap();

        let mut seen: Vec<Progress> = Vec::new();
        let path = ensure(&TINY, &dir, &mut |progress| seen.push(progress))
            .await
            .unwrap();

        assert_eq!(path, dir.join(TINY.filename));
        assert_eq!(seen.len(), 2, "expected a start and a verifying callback");
        assert_eq!(seen[0].downloaded_bytes, TINY.bytes);
        assert!(!seen[0].verifying);
        assert!(seen[1].verifying);
        assert!(!dir.join(format!("{}.part", TINY.filename)).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_wrong_digest_is_a_checksum_error_and_deletes_the_bad_bytes() {
        let dir = temp_dir("bad-digest");
        let part = dir.join(format!("{}.part", TINY.filename));
        std::fs::write(&part, b"notzeros").unwrap();

        let error = ensure(&TINY, &dir, &mut |_| {}).await.unwrap_err();

        // Must be ModelChecksum, not ModelDownload: the UI shows a
        // security-flavoured screen for one and "try again" for the other.
        assert!(matches!(error, Error::Checksum { .. }), "got {error:?}");
        assert!(!dir.join(TINY.filename).exists(), "an unverified file was promoted");
        assert!(!part.exists(), "\"download again from scratch\" would resume bad bytes");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn an_oversized_part_file_is_discarded_rather_than_resumed() {
        // Resuming past the pinned size would surface as a checksum failure,
        // which reads as tampering. Start over instead.
        let dir = temp_dir("oversized");
        let part = dir.join(format!("{}.part", TINY.filename));
        std::fs::write(&part, [0u8; 32]).unwrap();

        let mut first: Option<Progress> = None;
        // The download then fails for lack of a network, which is fine — the
        // assertion is about what was reported before the request went out.
        let _ = ensure(&TINY, &dir, &mut |progress| {
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
        std::fs::remove_dir_all(&dir).ok();
    }
}
