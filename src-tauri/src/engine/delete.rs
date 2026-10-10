//! Deleting a downloaded whisper model from Settings → Speech (TUR-132).
//!
//! The models are hundreds of megabytes to gigabytes each, so a user who tried
//! a few needs a way to give the space back. Only a model that is installed and
//! not in use can go: the one `transcription.model` names would be needed by
//! the next recording, and a recording running now may have it open.

use std::path::{Path, PathBuf};

use stt::model::ModelSpec;
use tauri::{AppHandle, Manager as _};

use super::{Download, Downloads, ModelDirs, models_dir};
use crate::error::UiError;
use crate::folder_move::FolderGate;
use crate::recording::{Phase, Recorder};

/// Delete a whisper model's file (primary or stranded copy) and any `.part`
/// left from a download that never finished.
///
/// Takes the same two guards as a download, so a delete never runs beside a
/// download of the same model or a move of the meetings folder.
pub fn delete(app: &AppHandle, id: &str) -> Result<(), UiError> {
    let spec = whisper(id)?;
    let gate = app.state::<FolderGate>();
    let _writing = gate.begin_write()?;
    let downloads = app.state::<Downloads>();
    let Some(_claim) = downloads.claim(id) else {
        return Err(UiError::app(
            "download-already-running",
            "That model is downloading. Wait for it to finish, then delete it.",
        ));
    };
    let idle = app.state::<Recorder>().status().phase == Phase::Idle;
    check_not_in_use(id, &crate::config::transcription().model, idle)?;
    let dir = models_dir()?;
    let installed = Download::Whisper(spec).installed(&ModelDirs::around(Some(dir.clone())));
    remove(spec, installed.as_deref(), &dir)
}

/// A whisper model in the catalogue, or `unknown-model`. Parakeet is not
/// deletable from here.
fn whisper(id: &str) -> Result<&'static ModelSpec, UiError> {
    stt::model::find(id).ok_or_else(|| {
        UiError::app(
            "unknown-model",
            format!("meet-ai does not have a whisper model called {id:?} in its list."),
        )
    })
}

/// Refuse the picked model, and any model while a recording is not idle.
fn check_not_in_use(id: &str, picked: &str, idle: bool) -> Result<(), UiError> {
    if idle && id != picked {
        return Ok(());
    }
    Err(UiError::app(
        "model-in-use",
        "Pick another model first, and stop any recording.",
    ))
}

/// Remove `installed` (if any) with the digest file beside it (TUR-159), and
/// `<filename>.part` in `dir`. A file that is already gone is not an error.
fn remove(spec: &ModelSpec, installed: Option<&Path>, dir: &Path) -> Result<(), UiError> {
    let part: PathBuf = dir.join(format!("{}.part", spec.filename));
    let digest = installed.map(stt::model::digest_file);
    for path in installed
        .into_iter()
        .chain(digest.as_deref())
        .chain([part.as_path()])
    {
        match std::fs::remove_file(path) {
            Ok(()) => tracing::info!(model = spec.id, path = %path.display(), "deleted model file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "small.en-q5_1";

    #[test]
    fn the_file_and_its_part_are_removed() {
        let spec = whisper(ID).expect("in the catalogue");
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join(spec.filename);
        let part = dir.path().join(format!("{}.part", spec.filename));
        std::fs::write(&file, b"model").expect("write model");
        std::fs::write(&part, b"half").expect("write part");
        let digest = stt::model::digest_file(&file);
        std::fs::write(&digest, spec.sha256).expect("write digest");

        remove(spec, Some(&file), dir.path()).expect("deleted");

        assert!(!file.exists());
        assert!(!part.exists());
        assert!(!digest.exists());
    }

    #[test]
    fn nothing_on_disk_is_not_an_error() {
        let spec = whisper(ID).expect("in the catalogue");
        let dir = tempfile::tempdir().expect("temp dir");
        remove(spec, None, dir.path()).expect("nothing to delete is fine");
    }

    #[test]
    fn the_picked_model_is_refused() {
        let error = check_not_in_use(ID, ID, true).expect_err("picked");
        assert_eq!(error.kind, "model-in-use");
    }

    #[test]
    fn any_model_is_refused_while_recording() {
        let error = check_not_in_use(ID, "large-v3-turbo-q5_0", false).expect_err("recording");
        assert_eq!(error.kind, "model-in-use");
    }

    #[test]
    fn another_model_while_idle_is_allowed() {
        assert!(check_not_in_use(ID, "large-v3-turbo-q5_0", true).is_ok());
    }

    #[test]
    fn an_unknown_or_parakeet_id_is_unknown_model() {
        assert_eq!(whisper("nope").expect_err("unknown").kind, "unknown-model");
    }
}
