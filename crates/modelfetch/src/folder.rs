//! Models that are a folder of files rather than one (TUR-62: Parakeet).
//!
//! [`ensure_folder`] is [`crate::ensure`] once per file, into one folder, with
//! the progress of each file folded into one bar for the whole model. Every
//! guarantee of `ensure` holds per file: lazy, resumable, verified, pinned.

use std::path::{Path, PathBuf};

use stt::model::ModelSpec;

use crate::{Error, Progress};

/// Make sure every file in `files` is present and verified in `dir`,
/// downloading what is missing, one file after another.
///
/// Returns `dir`. `on_progress` sees the whole folder: `total_bytes` is every
/// file together and `downloaded_bytes` counts the files already finished. It
/// reads `verifying` while one file is being hashed, so a UI shows the same
/// indeterminate bar it shows for a single-file model.
pub async fn ensure_folder(
    files: &[ModelSpec],
    dir: &Path,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Error> {
    let total: u64 = files.iter().map(|file| file.bytes).sum();
    let mut finished = 0;
    for file in files {
        let mut forward = |progress: Progress| on_progress(overall(finished, total, progress));
        crate::ensure(file, dir, &mut forward).await?;
        finished += file.bytes;
    }
    Ok(dir.to_path_buf())
}

/// One file's progress as progress through the whole folder.
fn overall(finished: u64, total: u64, file: Progress) -> Progress {
    Progress {
        downloaded_bytes: finished.saturating_add(file.downloaded_bytes).min(total),
        total_bytes: total,
        verifying: file.verifying,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight zero bytes, the same stand-in `lib.rs`'s tests use.
    const ZEROS: &str = "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc";

    const FILES: &[ModelSpec] = &[
        ModelSpec {
            id: "test-folder one",
            filename: "one.onnx",
            url: "https://huggingface.co/does-not-exist/one.onnx",
            sha256: ZEROS,
            bytes: 8,
            facts: stt::model::ModelFacts::NONE,
        },
        ModelSpec {
            id: "test-folder two",
            filename: "two.txt",
            url: "https://huggingface.co/does-not-exist/two.txt",
            sha256: ZEROS,
            bytes: 8,
            facts: stt::model::ModelFacts::NONE,
        },
    ];

    #[test]
    fn a_file_partway_reads_as_partway_through_the_folder() {
        let file = |downloaded_bytes, verifying| Progress {
            downloaded_bytes,
            total_bytes: 600,
            verifying,
        };
        assert_eq!(
            overall(400, 1_000, file(300, false)),
            Progress {
                downloaded_bytes: 700,
                total_bytes: 1_000,
                verifying: false
            }
        );
        assert!(overall(0, 1_000, file(600, true)).verifying);
        // Never past the end, whatever a file reports.
        assert_eq!(
            overall(900, 1_000, file(600, false)).downloaded_bytes,
            1_000
        );
    }

    #[tokio::test]
    async fn an_installed_folder_is_returned_without_touching_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        for file in FILES {
            std::fs::write(tmp.path().join(file.filename), b"verified").unwrap();
        }
        let mut calls = 0;
        let dir = ensure_folder(FILES, tmp.path(), &mut |_| calls += 1)
            .await
            .unwrap();
        assert_eq!(dir, tmp.path());
        assert_eq!(calls, 0);
    }

    #[tokio::test]
    async fn finished_parts_are_verified_in_turn_with_one_bar_for_the_folder() {
        // Both files already fully downloaded as `.part`: each is verified and
        // promoted, no request goes out, and the bar counts the first file as
        // done while the second is checked.
        let tmp = tempfile::tempdir().unwrap();
        for file in FILES {
            std::fs::write(tmp.path().join(format!("{}.part", file.filename)), [0u8; 8]).unwrap();
        }
        let mut seen = Vec::new();
        ensure_folder(FILES, tmp.path(), &mut |progress| seen.push(progress))
            .await
            .unwrap();

        assert!(seen.iter().all(|progress| progress.total_bytes == 16));
        let downloaded: Vec<_> = seen
            .iter()
            .map(|p| (p.downloaded_bytes, p.verifying))
            .collect();
        assert_eq!(downloaded, [(8, false), (8, true), (16, false), (16, true)]);
        for file in FILES {
            assert!(tmp.path().join(file.filename).is_file(), "{}", file.id);
        }
    }

    #[tokio::test]
    async fn a_bad_file_stops_the_folder_with_its_own_checksum_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(FILES[0].filename), b"verified").unwrap();
        std::fs::write(
            tmp.path().join(format!("{}.part", FILES[1].filename)),
            b"notzeros",
        )
        .unwrap();
        match ensure_folder(FILES, tmp.path(), &mut |_| {}).await {
            Err(Error::Checksum { model, .. }) => assert_eq!(model, "test-folder two"),
            other => panic!("expected a checksum error, got {other:?}"),
        }
    }
}
