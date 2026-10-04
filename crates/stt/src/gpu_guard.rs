//! The GPU crash marker (TUR-61).
//!
//! whisper.cpp on a GPU can take the whole process down: ggml calls `abort()`
//! on a failed assert when video memory runs out (Handy #2114), and some
//! drivers crash inside their own code (Handy #2007). Neither is an error we
//! can catch, so this module notices after the fact instead:
//!
//! 1. Before whisper first starts on the GPU, [`arm`] writes a marker file
//!    ([`meeting_format::layout::GPU_CHECK_FILE`] in `.app/`).
//! 2. After the first successful decode, [`GpuGuard::passed`] removes it. A
//!    clean shutdown (the guard dropped) removes it too; a crash never runs
//!    that code, which is the point.
//! 3. A marker still there at the next start means the GPU took the app down:
//!    [`arm`] answers "CPU" and logs it. The marker stays, so every later run
//!    uses the CPU too, until the user deletes the file the log line names.
//!
//! No child process: a crash costs the recording that hit it, never the next
//! one. Only the first decode is covered; a crash later in a long meeting
//! leaves no marker (the follow-up is inference in a child process).
//!
//! Platform-free: plain files, so the whole rule is tested on every OS.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use meeting_format::layout::{APP_DIR, GPU_CHECK_FILE, MODELS_DIR};

/// The marker file inside `dir`.
pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join(GPU_CHECK_FILE)
}

/// Did whisper crash on the GPU before, as far as the marker in `dir` says?
pub fn crashed_before(dir: &Path) -> bool {
    marker_path(dir).is_file()
}

/// Where the marker for `model` goes: the `.app/` folder its `models/` folder
/// sits in, or the model's own folder when it is not in that layout (a CLI
/// given `--dir`).
pub fn dir_for_model(model: &Path) -> Option<PathBuf> {
    let models = model.parent()?;
    match models.parent() {
        Some(app)
            if models.file_name() == Some(OsStr::new(MODELS_DIR))
                && app.file_name() == Some(OsStr::new(APP_DIR)) =>
        {
            Some(app.to_path_buf())
        }
        _ => Some(models.to_path_buf()),
    }
}

/// What [`arm`] decided.
#[derive(Debug)]
pub struct GpuDecision {
    /// Start whisper on the GPU.
    pub use_gpu: bool,
    /// The marker to clear once the GPU has worked; `None` when there is
    /// nothing to clear (CPU, or the marker could not be written).
    pub guard: Option<Arc<GpuGuard>>,
}

/// Decide GPU or CPU for this start, writing the marker when it is GPU.
pub fn arm(dir: &Path) -> GpuDecision {
    let marker = marker_path(dir);
    if marker.is_file() {
        tracing::warn!(
            marker = %marker.display(),
            "whisper crashed on the GPU in an earlier run, so it runs on the CPU; \
             delete this file to try the GPU again"
        );
        return GpuDecision {
            use_gpu: false,
            guard: None,
        };
    }
    match std::fs::write(&marker, b"whisper is starting on the GPU\n") {
        Ok(()) => GpuDecision {
            use_gpu: true,
            guard: Some(Arc::new(GpuGuard {
                marker,
                cleared: AtomicBool::new(false),
            })),
        },
        Err(error) => {
            // Without the marker a crash goes unnoticed, but refusing the GPU
            // for a folder we cannot write to would be a slowdown for nothing.
            tracing::warn!(
                %error,
                marker = %marker.display(),
                "could not write the GPU crash marker; using the GPU without it"
            );
            GpuDecision {
                use_gpu: true,
                guard: None,
            }
        }
    }
}

/// An armed marker. Shared by the engine and its sessions.
#[derive(Debug)]
pub struct GpuGuard {
    marker: PathBuf,
    cleared: AtomicBool,
}

impl GpuGuard {
    /// The GPU worked (or the run ended without a crash): remove the marker.
    /// Only the first call touches the disk.
    pub fn passed(&self) {
        if self.cleared.swap(true, Ordering::AcqRel) {
            return;
        }
        match std::fs::remove_file(&self.marker) {
            Ok(()) => tracing::debug!("whisper ran on the GPU; crash marker removed"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                %error,
                marker = %self.marker.display(),
                "could not remove the GPU crash marker; the next run will use the CPU"
            ),
        }
    }
}

impl Drop for GpuGuard {
    fn drop(&mut self) {
        self.passed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_first_start_uses_the_gpu_and_leaves_a_marker_until_it_passes() {
        let dir = tempfile::tempdir().unwrap();
        let decision = arm(dir.path());
        assert!(decision.use_gpu);
        assert!(crashed_before(dir.path()), "the marker must be on disk");

        let guard = decision.guard.expect("an armed guard");
        guard.passed();
        assert!(!crashed_before(dir.path()));
        // A second pass is a no-op, not an error.
        guard.passed();
    }

    #[test]
    fn a_marker_left_by_a_crash_means_cpu_and_it_stays() {
        let dir = tempfile::tempdir().unwrap();
        // A run that crashed: armed, never passed, never dropped.
        std::mem::forget(arm(dir.path()).guard);

        let decision = arm(dir.path());
        assert!(!decision.use_gpu);
        assert!(decision.guard.is_none());
        assert!(crashed_before(dir.path()), "the next run must still see it");
    }

    #[test]
    fn a_clean_shutdown_clears_the_marker_even_with_no_decode() {
        let dir = tempfile::tempdir().unwrap();
        drop(arm(dir.path()));
        assert!(!crashed_before(dir.path()));
        assert!(arm(dir.path()).use_gpu);
    }

    #[test]
    fn a_folder_that_cannot_hold_the_marker_still_uses_the_gpu() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-there");
        let decision = arm(&missing);
        assert!(decision.use_gpu);
        assert!(decision.guard.is_none());
    }

    #[test]
    fn the_marker_sits_in_the_app_folder_of_a_model() {
        let root = Path::new("root");
        let model = meeting_format::layout::models_dir(root).join("ggml-small.en-q5_1.bin");
        assert_eq!(
            dir_for_model(&model),
            Some(meeting_format::layout::app_dir(root))
        );
        let elsewhere = Path::new("somewhere").join("ggml-small.en-q5_1.bin");
        assert_eq!(dir_for_model(&elsewhere), Some(PathBuf::from("somewhere")));
    }
}
