//! `meet-stt-model` — fetch and verify a whisper model, or the Parakeet folder.
//!
//! The whisper path needs a model on disk before it can do anything, and the
//! app's onboarding screen (SPEC §8.1, step 2 of `/onboarding`) calls the same
//! [`modelfetch::ensure`] this binary does. Having it as a CLI as well means
//! the Phase 1 gate — "read the same recording on both engines" — can be run
//! without the app, and means a failed download can be retried and inspected
//! by hand.
//!
//! ```text
//! meet-stt-model list
//! meet-stt-model get small.en-q5_1
//! meet-stt-model get large-v3-turbo-q5_0 --dir /tmp/models
//! meet-stt-model get parakeet-tdt-0.6b-v3
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use modelfetch::Progress;
use stt::model::{self, ModelSpec, parakeet::ParakeetModel};

/// A model the CLI can fetch: one whisper file, or the Parakeet folder.
#[derive(Clone, Copy)]
enum Target {
    Whisper(&'static ModelSpec),
    Parakeet(&'static ParakeetModel),
}

impl Target {
    /// Every model, whisper first, in catalogue order.
    fn all() -> impl Iterator<Item = Self> {
        model::MODELS
            .iter()
            .map(Self::Whisper)
            .chain([Self::Parakeet(&model::parakeet::PARAKEET_V3)])
    }

    fn find(id: &str) -> Option<Self> {
        model::find(id)
            .map(Self::Whisper)
            .or_else(|| model::parakeet::find(id).map(Self::Parakeet))
    }

    fn id(self) -> &'static str {
        match self {
            Self::Whisper(spec) => spec.id,
            Self::Parakeet(model) => model.id,
        }
    }

    fn bytes(self) -> u64 {
        match self {
            Self::Whisper(spec) => spec.bytes,
            Self::Parakeet(model) => model.bytes(),
        }
    }

    fn is_installed(self, dir: &Path) -> bool {
        match self {
            Self::Whisper(spec) => model::is_installed(spec, dir),
            Self::Parakeet(model) => model.is_installed(dir),
        }
    }
}

fn usage() -> &'static str {
    "meet-stt-model — download and verify speech models (whisper and Parakeet).

USAGE:
    meet-stt-model list
    meet-stt-model get <MODEL_ID> [--dir <PATH>]

The download is resumable and checksum-verified. Re-running `get` after an
interrupted attempt continues where it stopped; the file only appears under its
real name once its SHA-256 matches the pinned digest.

Models go to $MEET_AI_MEETINGS_ROOT/.app/models when that is set, otherwise
~/Meetings/.app/models. If you moved your meetings folder in the app, pass
--dir <that folder>/.app/models."
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("list") => {
            let dir = model::default_model_dir().ok();
            for target in Target::all() {
                let installed = dir.as_deref().is_some_and(|dir| target.is_installed(dir));
                println!(
                    "{:<24} {:>7.0} MB  {}",
                    target.id(),
                    target.bytes() as f64 / 1_000_000.0,
                    if installed {
                        "installed"
                    } else {
                        "not downloaded"
                    }
                );
            }
            ExitCode::SUCCESS
        }

        Some("get") => {
            let Some(id) = args.get(1) else {
                eprintln!("{}", usage());
                return ExitCode::FAILURE;
            };
            let Some(target) = Target::find(id) else {
                eprintln!("unknown model {id:?}. Try `meet-stt-model list`.");
                return ExitCode::FAILURE;
            };

            let dir = match args.iter().position(|a| a == "--dir") {
                Some(index) => match args.get(index + 1) {
                    Some(path) => PathBuf::from(path),
                    None => {
                        eprintln!("--dir needs a path");
                        return ExitCode::FAILURE;
                    }
                },
                None => match model::default_model_dir() {
                    Ok(dir) => dir,
                    Err(error) => {
                        eprintln!("{error}");
                        return ExitCode::FAILURE;
                    }
                },
            };

            // Rewrite one line rather than scrolling, once per percent.
            let mut last_percent = u64::MAX;
            let mut on_progress = move |progress: Progress| {
                if progress.verifying {
                    eprint!("\rverifying checksum...            ");
                    let _ = std::io::stderr().flush();
                    return;
                }
                let percent = (progress.fraction() * 100.0) as u64;
                if percent != last_percent {
                    last_percent = percent;
                    eprint!(
                        "\r{percent:>3}%  {:.0}/{:.0} MB",
                        progress.downloaded_bytes as f64 / 1_000_000.0,
                        progress.total_bytes as f64 / 1_000_000.0
                    );
                    let _ = std::io::stderr().flush();
                }
            };

            // Nothing here cancels: Ctrl-C ends the process, and the `.part`
            // files it leaves are what the next `get` resumes.
            let never = AtomicBool::new(false);
            let fetched = match target {
                Target::Whisper(spec) => {
                    modelfetch::ensure(spec, &dir, &never, &mut on_progress).await
                }
                Target::Parakeet(model) => {
                    modelfetch::ensure_folder(
                        model.files,
                        &model.dir(&dir),
                        &never,
                        &mut on_progress,
                    )
                    .await
                }
            };
            match fetched {
                Ok(path) => {
                    eprintln!("\rverified                         ");
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("\n{error}");
                    ExitCode::FAILURE
                }
            }
        }

        _ => {
            eprintln!("{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_get_know_parakeet_as_well_as_every_whisper_model() {
        let ids: Vec<_> = Target::all().map(Target::id).collect();
        assert!(ids.contains(&"parakeet-tdt-0.6b-v3"), "{ids:?}");
        for spec in model::MODELS {
            assert!(ids.contains(&spec.id), "{ids:?}");
        }
        let parakeet = Target::find("parakeet-tdt-0.6b-v3").map(Target::id);
        assert_eq!(parakeet, Some("parakeet-tdt-0.6b-v3"));
        assert_eq!(
            Target::find("parakeet-tdt-0.6b-v3").map(Target::bytes),
            Some(model::parakeet::PARAKEET_V3.bytes())
        );
        assert!(Target::find("not-a-model").is_none());
    }
}
