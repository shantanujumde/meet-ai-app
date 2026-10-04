//! The Parakeet model (TUR-62): NVIDIA's `parakeet-tdt-0.6b-v3`, as the ONNX
//! export `parakeet-rs` reads.
//!
//! Unlike a whisper model it is not one file but a folder of three: the
//! encoder, the decoder/joint network and the token list. Each is a
//! [`ModelSpec`] of its own, so `modelfetch::ensure` downloads, resumes and
//! verifies it exactly as it does a whisper file, into [`ParakeetModel::dir`].
//! The model counts as installed only when all three are there under their
//! finished names, which `ensure` only gives a file after its SHA-256 matched.
//!
//! The files are the int8 export, the one Handy runs on the CPU: 652 MB for the
//! encoder against 2.4 GB for full precision.
//!
//! Sizes and digests were read from the Hugging Face API for repository
//! `istupakov/parakeet-tdt-0.6b-v3-onnx` at commit
//! `8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce` on 2026-10-04 (the LFS object
//! ids). `vocab.txt` is a plain git file there, so the API gives no SHA-256 for
//! it; its digest was computed from the 94 KB text file at that commit, whose
//! git blob id matched the API's (`fc43e1c7`). The URLs pin the same commit, so
//! a later push upstream cannot change what is fetched.
//!
//! Licence: CC-BY-4.0, from NVIDIA. Attribution is required, so [`CREDIT`] is
//! shown in Settings, About, and in `THIRD_PARTY_NOTICES.md`.

use std::path::{Path, PathBuf};

use super::{ModelFacts, ModelSpec};

/// The Parakeet model this build can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParakeetModel {
    /// The id the window and the download command use.
    pub id: &'static str,
    /// The folder under the models directory that holds [`Self::files`].
    pub dir_name: &'static str,
    /// The plain-word name for the Settings row.
    pub display_name: &'static str,
    /// One line on when to pick it.
    pub good_for: &'static str,
    /// Every file the folder needs, each pinned and verified on its own.
    pub files: &'static [ModelSpec],
    /// The languages it understands, `(code, English name)`, from NVIDIA's
    /// model card.
    pub languages: &'static [(&'static str, &'static str)],
}

impl ParakeetModel {
    /// Where the folder lives under `models_dir` (`<root>/.app/models`).
    pub fn dir(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(self.dir_name)
    }

    /// All the files together, for the download size on the row.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }

    /// Is every file present under its finished name?
    ///
    /// A `.part` file left by an interrupted download does not count; it is
    /// what the next `modelfetch::ensure` resumes.
    pub fn is_installed(&self, models_dir: &Path) -> bool {
        let dir = self.dir(models_dir);
        self.files
            .iter()
            .all(|file| super::is_installed(file, &dir))
    }
}

/// The model's three files. `id` names the file in a checksum error.
const FILES: &[ModelSpec] = &[
    ModelSpec {
        id: "parakeet-tdt-0.6b-v3 encoder",
        filename: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx",
        sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        bytes: 652_183_999,
        facts: ModelFacts::NONE,
    },
    ModelSpec {
        id: "parakeet-tdt-0.6b-v3 decoder",
        filename: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx",
        sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        bytes: 18_202_004,
        facts: ModelFacts::NONE,
    },
    ModelSpec {
        id: "parakeet-tdt-0.6b-v3 vocabulary",
        filename: "vocab.txt",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        bytes: 93_939,
        facts: ModelFacts::NONE,
    },
];

/// The 25 languages on NVIDIA's model card, in its order.
const LANGUAGES: &[(&str, &str)] = &[
    ("bg", "Bulgarian"),
    ("hr", "Croatian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("et", "Estonian"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("de", "German"),
    ("el", "Greek"),
    ("hu", "Hungarian"),
    ("it", "Italian"),
    ("lv", "Latvian"),
    ("lt", "Lithuanian"),
    ("mt", "Maltese"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ro", "Romanian"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("es", "Spanish"),
    ("sv", "Swedish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
];

/// `nvidia/parakeet-tdt-0.6b-v3`, int8.
pub const PARAKEET_V3: ParakeetModel = ParakeetModel {
    id: "parakeet-tdt-0.6b-v3",
    dir_name: "parakeet-tdt-0.6b-v3-int8",
    display_name: "Parakeet (25 European languages)",
    good_for: "Fast on a computer without a graphics card, so live captions keep up. \
               Understands 25 European languages, English included. Detects the language \
               by itself.",
    files: FILES,
    languages: LANGUAGES,
};

/// Look up the Parakeet model by its id.
pub fn find(id: &str) -> Option<&'static ParakeetModel> {
    (id == PARAKEET_V3.id).then_some(&PARAKEET_V3)
}

/// The attribution CC-BY-4.0 requires, for Settings, About.
pub const CREDIT: &str = "Parakeet speech model: parakeet-tdt-0.6b-v3 by NVIDIA, used under the \
                          Creative Commons Attribution 4.0 licence (CC-BY-4.0). ONNX export by \
                          istupakov. Not changed by meet-ai.";

/// Where the model and its licence are published, for the About link.
pub const CREDIT_URL: &str = "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3";

#[cfg(test)]
mod tests {
    use super::*;

    /// The commit every URL is pinned to.
    const REVISION: &str = "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";

    #[test]
    fn every_file_is_pinned_to_one_commit_with_a_real_digest() {
        for file in PARAKEET_V3.files {
            assert_eq!(file.sha256.len(), 64, "{}", file.id);
            assert!(
                file.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{}",
                file.id
            );
            let prefix = format!(
                "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/{REVISION}/"
            );
            assert!(file.url.starts_with(&prefix), "{} is not pinned", file.id);
            assert!(file.url.ends_with(file.filename), "{}", file.id);
            assert!(file.bytes > 0, "{}", file.id);
        }
    }

    #[test]
    fn the_folder_holds_the_files_parakeet_rs_looks_for() {
        // parakeet-rs 0.3.8 `ParakeetTDT::from_pretrained` wants a folder with
        // an encoder, a decoder_joint and `vocab.txt`; these are names from
        // its candidate lists.
        let names: Vec<_> = PARAKEET_V3.files.iter().map(|f| f.filename).collect();
        assert_eq!(
            names,
            [
                "encoder-model.int8.onnx",
                "decoder_joint-model.int8.onnx",
                "vocab.txt"
            ]
        );
        assert_eq!(PARAKEET_V3.bytes(), 670_479_942);
    }

    #[test]
    fn installed_means_every_file_under_its_finished_name() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path();
        assert!(!PARAKEET_V3.is_installed(models));

        let dir = PARAKEET_V3.dir(models);
        std::fs::create_dir_all(&dir).unwrap();
        let (last, rest) = PARAKEET_V3.files.split_last().unwrap();
        for file in rest {
            std::fs::write(dir.join(file.filename), b"stand-in").unwrap();
        }
        std::fs::write(dir.join(format!("{}.part", last.filename)), b"half").unwrap();
        assert!(!PARAKEET_V3.is_installed(models), "a .part is not finished");

        std::fs::write(dir.join(last.filename), b"stand-in").unwrap();
        assert!(PARAKEET_V3.is_installed(models));
    }

    #[test]
    fn the_row_speaks_plain_words_and_credits_nvidia() {
        assert_eq!(find(PARAKEET_V3.id), Some(&PARAKEET_V3));
        assert_eq!(find("small.en-q5_1"), None);
        assert_eq!(PARAKEET_V3.languages.len(), 25);
        assert!(PARAKEET_V3.good_for.contains("25 European languages"));
        for jargon in ["onnx", "int8", "tdt", "0.6b"] {
            assert!(
                !PARAKEET_V3.display_name.to_lowercase().contains(jargon)
                    && !PARAKEET_V3.good_for.to_lowercase().contains(jargon),
                "{jargon}"
            );
        }
        assert!(CREDIT.contains("NVIDIA") && CREDIT.contains("CC-BY-4.0"));
        assert!(CREDIT_URL.ends_with("nvidia/parakeet-tdt-0.6b-v3"));
    }
}
