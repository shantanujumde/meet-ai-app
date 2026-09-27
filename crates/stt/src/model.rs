//! The catalogue of whisper models the fallback engine can use.
//!
//! Pure data and filesystem lookups: which models exist, where they live, and
//! whether one is already on disk. That is everything [`crate::registry`]
//! needs to answer "what can this machine do right now, offline".
//!
//! **Downloading is deliberately not here.** It lives in the `modelfetch`
//! crate, so `crates/stt` has no HTTP stack anywhere in its dependency graph
//! and transcription cannot reach the network even by accident (L9/L10/L11).
//! The pinned URL and SHA-256 stay here, next to the rest of the catalogue,
//! because they describe *what* a model is rather than how it is fetched.
//!
//! On macOS 26+ nothing is ever downloaded at all — Apple's engine ships with
//! the OS and the first-run download is zero bytes (SPEC §2.4).

use std::path::{Path, PathBuf};

use crate::Error;

/// A model we are willing to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// The id used in `config.jsonc` (`transcription.model`).
    pub id: &'static str,
    /// Filename on disk, inside `~/Meetings/.app/models/`.
    pub filename: &'static str,
    /// Pinned download URL.
    pub url: &'static str,
    /// Expected SHA-256 of the finished file, lowercase hex.
    pub sha256: &'static str,
    /// Expected size in bytes, for progress reporting and a cheap sanity check.
    pub bytes: u64,
}

/// The models SPEC §2.4 names.
///
/// URLs point at `resolve/main` on Hugging Face, which is the canonical
/// distribution point for `whisper.cpp` GGML weights. The digests are the LFS
/// object ids Hugging Face publishes for these exact files, read from its API
/// on 2026-09-27 — not computed from a local download, which would only prove
/// the bytes matched themselves.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "small.en-q5_1",
        filename: "ggml-small.en-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin",
        sha256: "bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30",
        bytes: 190_098_681,
    },
    ModelSpec {
        id: "large-v3-turbo-q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        // SPEC §2.4 estimates ~1.6 GB for this file. The real q5_0 turbo build
        // is 574 MB; the 1.6 GB figure belongs to an unquantized large-v3.
        bytes: 574_041_195,
    },
];

/// Look up a model by its `config.jsonc` id.
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|model| model.id == id)
}

/// Where models live: `~/Meetings/.app/models/` (SPEC §3.1).
///
/// Built with `dirs` + `PathBuf::join` and no literal `~`, per the Windows seam
/// in SPEC §8.2.
pub fn default_model_dir() -> Result<PathBuf, Error> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Engine("could not determine the home directory".into()))?;
    Ok(home.join("Meetings").join(".app").join("models"))
}

/// Is this model already present and verified?
///
/// Only checks for the finished file. A stale `.part` is not "present"; it is
/// the thing `modelfetch::ensure` resumes.
pub fn is_installed(spec: &ModelSpec, dir: &Path) -> bool {
    dir.join(spec.filename).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pinned_model_has_a_plausible_digest_and_https_url() {
        for spec in MODELS {
            assert_eq!(spec.sha256.len(), 64, "{} digest is not 32 bytes", spec.id);
            assert!(
                spec.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} digest is not hex",
                spec.id
            );
            assert!(
                spec.url
                    .starts_with("https://huggingface.co/ggerganov/whisper.cpp/"),
                "{} is not pinned to the expected host",
                spec.id
            );
            assert!(spec.bytes > 0);
        }
    }

    #[test]
    fn models_are_found_by_their_config_id() {
        assert!(find("small.en-q5_1").is_some());
        assert!(find("large-v3-turbo-q5_0").is_some());
        assert!(find("not-a-model").is_none());
    }

    #[test]
    fn the_model_directory_is_under_the_meetings_root() {
        let dir = default_model_dir().unwrap();
        assert!(dir.ends_with("Meetings/.app/models"), "{}", dir.display());
    }
}
