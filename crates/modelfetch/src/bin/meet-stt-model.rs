//! `meet-stt-model` — fetch and verify a whisper model.
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
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use modelfetch::Progress;
use stt::model;

fn usage() -> &'static str {
    "meet-stt-model — download and verify whisper models.

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
            for spec in model::MODELS {
                let dir = model::default_model_dir().ok();
                let installed = dir
                    .as_deref()
                    .map(|dir| model::is_installed(spec, dir))
                    .unwrap_or(false);
                println!(
                    "{:<24} {:>7.0} MB  {}",
                    spec.id,
                    spec.bytes as f64 / 1_000_000.0,
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
            let Some(spec) = model::find(id) else {
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

            // Rewrite one line rather than scrolling: a 574 MB download emits a
            // progress callback per chunk.
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

            match modelfetch::ensure(spec, &dir, &mut on_progress).await {
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
