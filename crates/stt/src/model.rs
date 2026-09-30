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
    /// Filename on disk, inside [`model_dir`].
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

/// Where models live: `<meetings root>/.app/models/` (SPEC §3.1).
///
/// The root is an argument because it is not always `~/Meetings`: the user can
/// move it from Settings, and `change_root` carries `.app/` — models included —
/// along with every meeting. A directory derived here from `~` would keep
/// pointing at the old place after that move, so a model the user had already
/// downloaded would read as missing and be fetched a second time into a folder
/// the app no longer owns. The app passes its own resolved root
/// (`meetings::root()`); only tools with no app to ask use
/// [`default_model_dir`].
pub fn model_dir(meetings_root: &Path) -> PathBuf {
    meetings_root.join(".app").join("models")
}

/// [`model_dir`] under the fallback root, for callers that have no app to ask.
///
/// That is the `meet-stt-model` CLI, the `offline_meeting` example and the
/// tests — nothing that runs inside the app. The fallback root is
/// `MEET_AI_MEETINGS_ROOT` when set (the same override the app's
/// `meetings::root()` honours first), otherwise `~/Meetings`. It cannot see a
/// folder the user picked in Settings: that choice is recorded by the app, and
/// reading it from here would put a second copy of the app's root rules in a
/// crate that should not know them. Pass `--dir` to the CLI in that case.
///
/// Built with `dirs` + `PathBuf::join` and no literal `~`, per the Windows seam
/// in SPEC §8.2.
pub fn default_model_dir() -> Result<PathBuf, Error> {
    let root = fallback_meetings_root(std::env::var_os(MEETINGS_ROOT_ENV), dirs::home_dir())?;
    Ok(model_dir(&root))
}

/// The variable both this crate and the app read to point meet-ai at a
/// different meetings root — a fixture folder in development, most often.
pub const MEETINGS_ROOT_ENV: &str = "MEET_AI_MEETINGS_ROOT";

/// The fallback root, with its two inputs passed in so the precedence can be
/// tested without mutating the process environment under parallel tests.
fn fallback_meetings_root(
    env_override: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    if let Some(custom) = env_override.filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(custom));
    }
    let home =
        home.ok_or_else(|| Error::Engine("could not determine the home directory".into()))?;
    Ok(home.join("Meetings"))
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
    fn the_model_directory_is_under_whatever_root_it_is_given() {
        // A root the user picked in Settings, nowhere near `~/Meetings`. The
        // models must follow it, or `change_root` strands them (Phase 1 bug 3).
        let root = Path::new("/Volumes/Archive/Work meetings");
        assert_eq!(
            model_dir(root),
            Path::new("/Volumes/Archive/Work meetings/.app/models")
        );
    }

    #[test]
    fn the_fallback_root_prefers_the_env_override_over_home() {
        let home = Some(PathBuf::from("/Users/someone"));
        assert_eq!(
            fallback_meetings_root(Some("/tmp/fixture-root".into()), home.clone()).unwrap(),
            Path::new("/tmp/fixture-root")
        );
        assert_eq!(
            fallback_meetings_root(None, home.clone()).unwrap(),
            Path::new("/Users/someone/Meetings")
        );
        // An empty variable is "unset", not "the current directory".
        assert_eq!(
            fallback_meetings_root(Some("".into()), home).unwrap(),
            Path::new("/Users/someone/Meetings")
        );
        assert!(fallback_meetings_root(None, None).is_err());
    }

    #[test]
    fn the_fallback_model_directory_is_under_a_meetings_root() {
        let dir = default_model_dir().unwrap();
        assert!(dir.ends_with(".app/models"), "{}", dir.display());
    }
}
