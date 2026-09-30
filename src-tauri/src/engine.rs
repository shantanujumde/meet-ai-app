//! The settings / engine screen and the model download screen.
//!
//! One rule shapes this whole file: **the cheap question and the expensive one
//! are different commands.**
//!
//! * [`Environment::discover`] is filesystem-only and sub-millisecond. Block on
//!   it freely — the screen can be built out of its answer before paint.
//! * `registry::resolve` spawns `meet-stt --probe`, measured at a median of
//!   ~160 ms. That is far too slow to hold a route open and far too fast to
//!   deserve a full-screen spinner, so the screen renders immediately with a
//!   "Checking…" row and fills it in.
//!
//! Collapsing the two into one command would force the slower behaviour on the
//! whole screen, which is the mistake this split exists to prevent.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use stt::registry::{self, Environment, Preference};
use tauri::{AppHandle, Emitter as _};

use crate::error::UiError;
use crate::meetings;

/// SPEC §3.5's defaults.
///
/// `DEFAULT_MODEL` is only the fallback: `crate::config::transcription` reads
/// `transcription.model` (and `transcription.engine`) from `config.jsonc` and
/// wins when it is set. `DEFAULT_LOCALE` has no config key yet —
/// `transcription.language` is Phase 6, same as every other key in §3.5 beside
/// `engine` and `model`.
pub const DEFAULT_LOCALE: &str = "en-US";
pub const DEFAULT_MODEL: &str = "large-v3-turbo-q5_0";

/// Progress events for a model download.
pub const MODEL_PROGRESS_EVENT: &str = "model://progress";

/// What the filesystem says, with no subprocess involved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentView {
    /// Absolute path to the `meet-stt` sidecar, if it was found. `None` means
    /// Apple's engine is not reachable at all on this install.
    pub sidecar: Option<String>,
    /// Absolute path to a downloaded whisper model, if one is present.
    pub whisper_model: Option<String>,
    pub locale: String,
    pub model_id: String,
    /// `<meetings root>/.app/models/`, so the screen can name where downloads
    /// land.
    pub models_dir: Option<String>,
}

/// Which engine this Mac will actually use, and why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionView {
    /// `apple-speech` or `whisper`. The canonical names from `stt::registry`.
    pub engine: &'static str,
    /// A whole sentence, written by the registry. Shown as-is.
    pub reason: String,
}

/// One row on the model download screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub id: &'static str,
    pub filename: &'static str,
    /// The pinned size. SPEC §2.4's "~1.6 GB" is stale — amendment A4 corrects
    /// it to 574 MB for turbo and 190 MB for small.en — so the screen reads
    /// `spec.bytes` rather than quoting either document.
    pub bytes: u64,
    pub installed: bool,
}

/// Progress for one model, as emitted on [`MODEL_PROGRESS_EVENT`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// The bytes are all here and the SHA-256 is being computed. Hashing has no
    /// sub-progress, so the bar must go indeterminate rather than sit at 100%
    /// looking frozen.
    pub verifying: bool,
}

/// Which models are downloading right now.
///
/// Two `ensure` calls for the same model would resume the same `.part` file
/// from two directions and race each other to the atomic rename. One in-flight
/// download per model, enforced here rather than hoped for in the UI.
#[derive(Default)]
pub struct Downloads {
    in_flight: Mutex<HashSet<String>>,
}

impl Downloads {
    /// Claim a model, or find out someone else already has it (`None`).
    ///
    /// The claim is released when the returned [`Claim`] drops, not by a call
    /// the caller has to remember. A `release` written after the `.await` never
    /// ran if the download panicked or its future was dropped part-way, and
    /// the model then read as "already downloading" until the app restarted —
    /// a retry button that could never work again.
    pub fn claim(&self, id: &str) -> Option<Claim<'_>> {
        self.lock().insert(id.to_string()).then(|| Claim {
            downloads: self,
            id: id.to_string(),
        })
    }

    fn release(&self, id: &str) {
        self.lock().remove(id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.in_flight
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One model's download slot, held for as long as the download runs.
///
/// Dropping it frees the slot however the holder ends: success, an error, a
/// panic unwinding through it, or the command future being dropped.
#[must_use = "the claim is released as soon as it is dropped"]
pub struct Claim<'a> {
    downloads: &'a Downloads,
    id: String,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.downloads.release(&self.id);
    }
}

/// Where this app keeps whisper models: `.app/models` under the meetings root
/// the user actually chose.
///
/// Not `stt::model::default_model_dir`, which only knows `~/Meetings`. After a
/// move from Settings that would send the screen, the catalogue and every
/// download back to the old folder, and a model `change_root` had carried
/// across would read as missing.
fn models_dir() -> Result<PathBuf, UiError> {
    meetings::root().map(|root| stt::model::model_dir(&root))
}

/// The cheap half. Filesystem only; safe to block a route on.
pub fn environment(locale: &str, model_id: &str) -> EnvironmentView {
    let models_dir = models_dir().ok();
    let discovered = Environment::discover_in(models_dir.as_deref(), locale, model_id);
    EnvironmentView {
        sidecar: discovered
            .sidecar
            .as_ref()
            .map(|path| path.display().to_string()),
        whisper_model: discovered
            .whisper_model
            .as_ref()
            .map(|path| path.display().to_string()),
        locale: discovered.locale,
        model_id: model_id.to_string(),
        models_dir: models_dir.map(|dir| dir.display().to_string()),
    }
}

/// The expensive half. Runs `meet-stt --probe`; call it off the render path.
pub fn resolve(
    preference: Preference,
    locale: &str,
    model_id: &str,
) -> Result<SelectionView, UiError> {
    let models_dir = models_dir().ok();
    let environment = Environment::discover_in(models_dir.as_deref(), locale, model_id);
    let selection = registry::resolve(preference, &environment)?;
    Ok(SelectionView {
        engine: selection.engine.name(),
        reason: selection.reason,
    })
}

/// The pinned model catalogue plus whether each one is already on disk.
pub fn catalogue() -> Vec<ModelView> {
    let dir = models_dir().ok();
    stt::model::MODELS
        .iter()
        .map(|spec| ModelView {
            id: spec.id,
            filename: spec.filename,
            bytes: spec.bytes,
            installed: dir
                .as_ref()
                .map(|dir| stt::model::is_installed(spec, dir))
                .unwrap_or(false),
        })
        .collect()
}

/// Download a model, reporting progress as Tauri events.
///
/// `modelfetch::ensure` fires `on_progress` at least twice for any real
/// download — once before the first request goes out, and once when
/// verification starts — precisely so a UI never has to guess whether a 0% bar
/// means "connecting" or "frozen". Both are forwarded.
pub async fn download(app: AppHandle, model_id: String) -> Result<String, UiError> {
    let spec = stt::model::find(&model_id).ok_or_else(|| {
        UiError::app(
            "unknown-model",
            format!("meet-ai does not have a model called {model_id:?} in its list."),
        )
    })?;
    let dir = models_dir()?;

    // `modelfetch::ensure` takes `&mut dyn FnMut(Progress)`, which is not
    // `Send`, so the future it returns is not `Send` and cannot be awaited
    // inside a Tauri command. Give it a thread and a current-thread runtime
    // rather than making the callback `Send` — that would mean wrapping
    // something only ever touched from one place in an `Arc<Mutex<_>>` to
    // satisfy a bound, not to fix a race.
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| UiError::app("runtime", error.to_string()))?;

        runtime.block_on(async move {
            let mut on_progress = |progress: modelfetch::Progress| {
                let event = ProgressEvent {
                    model_id: model_id.clone(),
                    downloaded_bytes: progress.downloaded_bytes,
                    total_bytes: progress.total_bytes,
                    verifying: progress.verifying,
                };
                if let Err(error) = app.emit(MODEL_PROGRESS_EVENT, &event) {
                    tracing::warn!(%error, "could not report download progress to the window");
                }
            };

            modelfetch::ensure(spec, &dir, &mut on_progress)
                .await
                .map(|path| path.display().to_string())
                .map_err(UiError::from)
        })
    })
    .await
    .map_err(|error| UiError::app("download-task-failed", error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_reports_the_pinned_sizes_not_the_stale_spec_figures() {
        let models = catalogue();
        let by_id = |id: &str| {
            models
                .iter()
                .find(|model| model.id == id)
                .unwrap_or_else(|| panic!("{id} must be in the catalogue"))
        };
        // SPEC §2.4 says ~1.6 GB for turbo and ~180 MB for small.en. A4
        // corrects both. The screen must show the corrected numbers.
        assert_eq!(by_id("large-v3-turbo-q5_0").bytes, 574_041_195);
        assert_eq!(by_id("small.en-q5_1").bytes, 190_098_681);
    }

    #[test]
    fn only_one_download_per_model_can_be_in_flight() {
        let downloads = Downloads::default();
        let small = downloads.claim("small.en-q5_1");
        assert!(small.is_some());
        assert!(
            downloads.claim("small.en-q5_1").is_none(),
            "a second claim must fail, or two writers race the same .part file"
        );
        // A different model is unaffected.
        let turbo = downloads.claim("large-v3-turbo-q5_0");
        assert!(turbo.is_some());

        drop(small);
        assert!(!downloads.lock().contains("small.en-q5_1"));
        assert!(
            downloads.lock().contains("large-v3-turbo-q5_0"),
            "releasing one model must not release the other"
        );
        drop(turbo);
    }

    #[test]
    fn a_download_that_panics_still_gives_its_claim_back() {
        // The release used to be a line after the `.await`, so a panic skipped
        // it and the model stayed "already downloading" until a restart.
        let downloads = Downloads::default();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _claim = downloads.claim("small.en-q5_1").expect("free to claim");
            panic!("the download blew up");
        }));
        assert!(unwound.is_err());
        assert!(
            downloads.claim("small.en-q5_1").is_some(),
            "the claim must be free again after the panic"
        );
    }

    #[test]
    fn discovering_the_environment_never_needs_the_network_or_a_subprocess() {
        // The whole point of the split: this is the call a route may block on.
        let view = environment(DEFAULT_LOCALE, DEFAULT_MODEL);
        assert_eq!(view.locale, DEFAULT_LOCALE);
        assert_eq!(view.model_id, DEFAULT_MODEL);
    }
}
