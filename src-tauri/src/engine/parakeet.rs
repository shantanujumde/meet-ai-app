//! The Parakeet model on the Settings screen (TUR-62): whether it is
//! downloaded, its row's facts, and downloading it.
//!
//! The model is a folder of three files (`stt::model::parakeet`). It lives in
//! `.app/models/` beside the whisper models and follows the meetings root the
//! same way, stranded copy included (see [`super::ModelDirs`]).

use std::path::PathBuf;

use serde::Serialize;
use stt::model::parakeet::{CREDIT, CREDIT_URL, PARAKEET_V3, ParakeetModel};

use super::ModelDirs;

/// The Parakeet row's facts, from the Rust catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ParakeetModelView {
    /// The id `download_model` takes.
    pub id: &'static str,
    pub display_name: &'static str,
    pub good_for: &'static str,
    /// Every file together.
    #[specta(type = specta_typescript::Number)]
    pub bytes: u64,
    pub installed: bool,
    /// ONNX Runtime is here to run it. The Windows installer ships
    /// `onnxruntime.dll` (TUR-104); false only if that file is missing, where
    /// a download would not help, so none is offered.
    pub runtime_ready: bool,
    /// English names, in the model card's order, for the (i) button.
    pub languages: Vec<&'static str>,
}

/// One model licence the app must credit (Settings, About).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCredit {
    /// The sentence to show, written in `stt`.
    pub text: &'static str,
    /// Where the model and its licence are published.
    pub url: &'static str,
}

/// The Parakeet folder, if every file of it is downloaded: from the meetings
/// root in use, else a stranded copy under the old default root.
pub(super) fn installed(dirs: &ModelDirs) -> Option<PathBuf> {
    installed_in(&PARAKEET_V3, dirs)
}

fn installed_in(model: &ParakeetModel, dirs: &ModelDirs) -> Option<PathBuf> {
    [dirs.primary.as_deref(), dirs.stranded.as_deref()]
        .into_iter()
        .flatten()
        .find(|dir| model.is_installed(dir))
        .map(|dir| model.dir(dir))
}

/// The row as it stands on disk now.
pub(super) fn view(dirs: &ModelDirs) -> ParakeetModelView {
    let model = &PARAKEET_V3;
    ParakeetModelView {
        id: model.id,
        display_name: model.display_name,
        good_for: model.good_for,
        bytes: model.bytes(),
        installed: installed(dirs).is_some(),
        runtime_ready: stt::registry::parakeet_runtime_missing().is_none(),
        languages: model.languages.iter().map(|(_, name)| *name).collect(),
    }
}

/// Every model licence that asks to be credited. Only Parakeet's so far
/// (CC-BY-4.0); whisper's MIT notice is in `THIRD_PARTY_NOTICES.md`.
pub fn credits() -> Vec<ModelCredit> {
    vec![ModelCredit {
        text: CREDIT,
        url: CREDIT_URL,
    }]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn install(models: &Path) {
        let dir = PARAKEET_V3.dir(models);
        std::fs::create_dir_all(&dir).unwrap();
        for file in PARAKEET_V3.files {
            std::fs::write(dir.join(file.filename), b"stand-in").unwrap();
        }
    }

    #[test]
    fn the_folder_is_found_under_the_chosen_root_first_then_a_stranded_one() {
        let scratch = tempfile::tempdir().unwrap();
        let primary = scratch.path().join("chosen/.app/models");
        let stranded = scratch.path().join("home/Meetings/.app/models");
        let dirs = ModelDirs {
            primary: Some(primary.clone()),
            stranded: Some(stranded.clone()),
        };
        assert_eq!(installed(&dirs), None);
        assert!(!view(&dirs).installed);

        install(&stranded);
        assert_eq!(installed(&dirs), Some(PARAKEET_V3.dir(&stranded)));

        install(&primary);
        assert_eq!(installed(&dirs), Some(PARAKEET_V3.dir(&primary)));
        assert!(view(&dirs).installed);
    }

    #[test]
    fn the_row_and_the_credit_come_from_the_catalogue() {
        let dirs = ModelDirs {
            primary: None,
            stranded: None,
        };
        let row = view(&dirs);
        assert_eq!(row.id, "parakeet-tdt-0.6b-v3");
        assert_eq!(row.bytes, PARAKEET_V3.bytes());
        assert_eq!(row.languages.len(), 25);
        assert!(row.languages.contains(&"English"));
        let credits = credits();
        assert_eq!(credits.len(), 1);
        assert!(credits[0].text.contains("CC-BY-4.0"));
        assert!(credits[0].url.starts_with("https://huggingface.co/nvidia/"));
    }
}
