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
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

use serde::Serialize;
use stt::model::ModelSpec;
use stt::registry::{self, Environment, Preference};
use tauri::{AppHandle, Emitter as _, Manager as _};

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
    /// ran if the download panicked, and the model then read as "already
    /// downloading" until the app restarted — a retry button that could never
    /// work again.
    ///
    /// [`download`] takes the claim *inside* its blocking task, on the thread
    /// that writes the `.part` file, not in the async command. A
    /// `spawn_blocking` task cannot be cancelled: if the command's future were
    /// dropped, a claim it held would be released while the thread kept
    /// writing, and a retry would start a second writer on the same file.
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

/// One model's download slot, held by the thread doing the download for as
/// long as that thread runs.
///
/// Dropping it frees the slot however the thread ends: success, an error, or a
/// panic unwinding through it.
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

/// Where to look for an installed model, best first.
///
/// `primary` is [`models_dir`]. `stranded` is the `~/Meetings/.app/models`
/// fallback, and only when that is somewhere else: before models followed the
/// meetings root, a user who had moved their meetings and then downloaded a
/// model got it there, under a root the app no longer uses. That copy is used
/// where it lies rather than moved — it is hundreds of megabytes, and a read
/// here must never turn into a copy — and new downloads always go to
/// `primary`.
struct ModelDirs {
    primary: Option<PathBuf>,
    stranded: Option<PathBuf>,
}

impl ModelDirs {
    fn discover() -> Self {
        Self::around(models_dir().ok())
    }

    fn around(primary: Option<PathBuf>) -> Self {
        let stranded = stt::model::default_model_dir()
            .ok()
            .filter(|fallback| Some(fallback) != primary.as_ref());
        Self { primary, stranded }
    }

    fn installed(&self, spec: &ModelSpec) -> Option<PathBuf> {
        locate(spec, self.primary.as_deref(), self.stranded.as_deref())
    }
}

/// The finished copy of `spec`, from `primary` if it is there, else from
/// `stranded`.
///
/// "Finished" is enough to trust it: `modelfetch::ensure` only renames the
/// `.part` file to its real name once the SHA-256 matches the pinned digest, so
/// a file under that name has already been verified. A stranded hit is logged
/// once per launch, so a user-submitted log explains why the model path sits
/// outside their meetings folder.
fn locate(spec: &ModelSpec, primary: Option<&Path>, stranded: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = primary
        && stt::model::is_installed(spec, dir)
    {
        return Some(dir.join(spec.filename));
    }
    let dir = stranded.filter(|dir| stt::model::is_installed(spec, dir))?;
    static STRANDED_NOTICE: Once = Once::new();
    STRANDED_NOTICE.call_once(|| {
        tracing::info!(
            model = spec.id,
            dir = %dir.display(),
            "using a model left in the old default folder rather than downloading it again"
        );
    });
    Some(dir.join(spec.filename))
}

/// What this Mac can do right now, looking for the whisper model where this
/// app keeps it (including a stranded copy — see [`ModelDirs`]).
///
/// The one discovery every caller in the app uses: the settings screen, the
/// probe and the live transcript. Two of them disagreeing is how Settings came
/// to show a model as installed that recording could not find.
pub fn discover(locale: &str, model_id: &str) -> Environment {
    discover_with(&ModelDirs::discover(), locale, model_id)
}

fn discover_with(dirs: &ModelDirs, locale: &str, model_id: &str) -> Environment {
    let mut environment = Environment::discover_in(None, locale, model_id);
    environment.whisper_model = stt::model::find(model_id).and_then(|spec| dirs.installed(spec));
    environment
}

/// The cheap half. Filesystem only; safe to block a route on.
pub fn environment(locale: &str, model_id: &str) -> EnvironmentView {
    let dirs = ModelDirs::discover();
    let discovered = discover_with(&dirs, locale, model_id);
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
        models_dir: dirs.primary.map(|dir| dir.display().to_string()),
    }
}

/// The expensive half. Runs `meet-stt --probe`; call it off the render path.
pub fn resolve(
    preference: Preference,
    locale: &str,
    model_id: &str,
) -> Result<SelectionView, UiError> {
    let environment = discover(locale, model_id);
    let selection = registry::resolve(preference, &environment)?;
    Ok(SelectionView {
        engine: selection.engine.name(),
        reason: selection.reason,
    })
}

/// The pinned model catalogue plus whether each one is already on disk.
pub fn catalogue() -> Vec<ModelView> {
    let dirs = ModelDirs::discover();
    stt::model::MODELS
        .iter()
        .map(|spec| ModelView {
            id: spec.id,
            filename: spec.filename,
            bytes: spec.bytes,
            installed: dirs.installed(spec).is_some(),
        })
        .collect()
}

/// Download a model, reporting progress as Tauri events.
///
/// `modelfetch::ensure` fires `on_progress` at least twice for any real
/// download — once before the first request goes out, and once when
/// verification starts — precisely so a UI never has to guess whether a 0% bar
/// means "connecting" or "frozen". Both are forwarded.
///
/// A model that is only present as a stranded copy (see [`ModelDirs`]) is
/// returned as-is rather than fetched again.
pub async fn download(app: AppHandle, model_id: String) -> Result<String, UiError> {
    let spec = stt::model::find(&model_id).ok_or_else(|| {
        UiError::app(
            "unknown-model",
            format!("meet-ai does not have a model called {model_id:?} in its list."),
        )
    })?;

    // `modelfetch::ensure` takes `&mut dyn FnMut(Progress)`, which is not
    // `Send`, so the future it returns is not `Send` and cannot be awaited
    // inside a Tauri command. Give it a thread and a current-thread runtime
    // rather than making the callback `Send` — that would mean wrapping
    // something only ever touched from one place in an `Arc<Mutex<_>>` to
    // satisfy a bound, not to fix a race.
    tauri::async_runtime::spawn_blocking(move || {
        // Claimed here, on the writing thread, so the slot is held exactly as
        // long as something can still write the `.part` file (see
        // `Downloads::claim`).
        let downloads = app.state::<Downloads>();
        let Some(_claim) = downloads.claim(&model_id) else {
            return Err(UiError::app(
                "download-already-running",
                "That model is already downloading.",
            ));
        };

        let dir = models_dir()?;
        let dirs = ModelDirs::around(Some(dir.clone()));
        if let Some(stranded) = dirs.installed(spec)
            && !stranded.starts_with(&dir)
        {
            return Ok(stranded.display().to_string());
        }

        let emitter = app.clone();
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
                if let Err(error) = emitter.emit(MODEL_PROGRESS_EVENT, &event) {
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

    /// A scratch directory removed on drop, so a failed assertion does not
    /// leave it behind.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("meet-ai-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn install(dir: &Path, spec: &ModelSpec) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(spec.filename), b"stands in for a verified model").unwrap();
    }

    #[test]
    fn a_model_stranded_in_the_old_default_folder_is_still_found() {
        // Moved the meetings folder, then downloaded — before models followed
        // the root, the download landed in `~/Meetings/.app/models`.
        let scratch = Scratch::new("engine-stranded");
        let primary = scratch.0.join("chosen/.app/models");
        let stranded = scratch.0.join("home/Meetings/.app/models");
        let spec = stt::model::find("small.en-q5_1").unwrap();
        install(&stranded, spec);

        assert_eq!(
            locate(spec, Some(&primary), Some(&stranded)),
            Some(stranded.join(spec.filename)),
            "the stranded copy is used where it lies"
        );
        assert!(
            !primary.join(spec.filename).exists(),
            "and never copied or moved"
        );

        let dirs = ModelDirs {
            primary: Some(primary.clone()),
            stranded: Some(stranded.clone()),
        };
        let environment = discover_with(&dirs, DEFAULT_LOCALE, spec.id);
        assert_eq!(
            environment.whisper_model,
            Some(stranded.join(spec.filename))
        );
    }

    #[test]
    fn the_chosen_root_wins_over_a_stranded_copy() {
        let scratch = Scratch::new("engine-primary-wins");
        let primary = scratch.0.join("chosen/.app/models");
        let stranded = scratch.0.join("home/Meetings/.app/models");
        let spec = stt::model::find("small.en-q5_1").unwrap();
        install(&primary, spec);
        install(&stranded, spec);

        assert_eq!(
            locate(spec, Some(&primary), Some(&stranded)),
            Some(primary.join(spec.filename))
        );
    }

    #[test]
    fn a_partial_download_is_not_a_stranded_model() {
        // Only the finished name counts: that is the file `ensure` renames into
        // place after the digest matches.
        let scratch = Scratch::new("engine-stranded-part");
        let stranded = scratch.0.join("home/Meetings/.app/models");
        let spec = stt::model::find("small.en-q5_1").unwrap();
        std::fs::create_dir_all(&stranded).unwrap();
        std::fs::write(
            stranded.join(format!("{}.part", spec.filename)),
            b"half a model",
        )
        .unwrap();

        assert_eq!(
            locate(spec, Some(&scratch.0.join("chosen")), Some(&stranded)),
            None
        );
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
