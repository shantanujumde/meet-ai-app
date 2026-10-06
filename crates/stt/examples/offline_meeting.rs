//! TUR-70: prove transcription never needs the network, from the command
//! line rather than from a structural argument.
//!
//! Runs the exact production path — [`stt::registry::select`] then
//! [`stt::transcribe_meeting`] — against a copy of a meeting folder, forcing
//! one engine so both can be exercised back to back. Compare the
//! `transcript.md` this prints with Wi-Fi on against a run with it off; L9/L10/L11
//! say they must be identical, because nothing on this path should have had
//! anything to fetch.
//!
//! ```text
//! cargo run -p stt --example offline_meeting -- probe --locale en-US
//! cargo run -p stt --example offline_meeting -- transcribe --engine apple \
//!     --meeting crates/audio/fixtures/two-speaker-60s --scratch /tmp/tur70-apple
//! cargo run -p stt --example offline_meeting -- transcribe --engine whisper \
//!     --meeting crates/audio/fixtures/two-speaker-60s --scratch /tmp/tur70-whisper \
//!     --model-id small.en-q5_1
//! ```
//!
//! stdout is the resulting `transcript.md` path and nothing else, so it can be
//! captured straight into a shell variable for diffing two runs.
//!
//! `--meeting` may be a real meeting folder (audio in `audio/`) or a flat
//! folder with `mic.wav` beside `system.wav` and `segments.json`, like the
//! fixtures. If `mic.wav` is in neither place the run fails with a message
//! naming both.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use meeting_format::{Channel, layout};
use stt::registry::{self, Preference};

const USAGE: &str = "\
usage:
  offline_meeting probe --locale LOCALE
  offline_meeting transcribe --engine apple|whisper --meeting DIR --scratch DIR
      [--locale LOCALE] [--model-id ID]
";

/// The folder that holds the meeting's audio: `audio/` in a real meeting, or
/// the folder itself when it is flat.
fn audio_source(meeting: &Path) -> Result<PathBuf, String> {
    let mic = Channel::Mic.wav_filename();
    let nested = layout::audio_dir(meeting);
    if nested.join(mic).is_file() {
        Ok(nested)
    } else if meeting.join(mic).is_file() {
        Ok(meeting.to_path_buf())
    } else {
        Err(format!(
            "no {mic} in {} or {}",
            meeting.display(),
            nested.display()
        ))
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    match mode.as_str() {
        "probe" => run_probe(&args[1..]),
        "transcribe" => run_transcribe(&args[1..]),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run_probe(args: &[String]) -> ExitCode {
    let locale = arg_value(args, "--locale").unwrap_or_else(|| "en-US".into());

    let Some(binary) = stt::apple::AppleEngine::discover() else {
        eprintln!("meet-stt not found (run `just sidecar` first)");
        return ExitCode::FAILURE;
    };

    match stt::apple::AppleEngine::probe(&binary, &locale) {
        Ok(probe) => {
            println!(
                "{}",
                serde_json::json!({
                    "available": probe.available,
                    "installed": probe.installed,
                    "locale": probe.locale,
                    "reason": probe.reason,
                    "os_version": probe.os_version,
                    "is_usable_offline": probe.is_usable_offline(),
                })
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_transcribe(args: &[String]) -> ExitCode {
    let Some(engine_name) = arg_value(args, "--engine") else {
        eprintln!("--engine apple|whisper is required\n\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let preference = match engine_name.as_str() {
        "apple" => Preference::AppleSpeech,
        "whisper" => Preference::Whisper,
        other => {
            eprintln!("unknown engine {other:?}, expected apple or whisper");
            return ExitCode::FAILURE;
        }
    };

    let Some(meeting) = arg_value(args, "--meeting").map(PathBuf::from) else {
        eprintln!("--meeting DIR is required\n\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let Some(scratch) = arg_value(args, "--scratch").map(PathBuf::from) else {
        eprintln!("--scratch DIR is required\n\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let locale = arg_value(args, "--locale").unwrap_or_else(|| "en-US".into());
    let model_id = arg_value(args, "--model-id").unwrap_or_else(|| "small.en-q5_1".into());

    std::fs::remove_dir_all(&scratch).ok();
    if let Err(error) = std::fs::create_dir_all(scratch.join("audio")) {
        eprintln!("could not create {}: {error}", scratch.display());
        return ExitCode::FAILURE;
    }
    let source = match audio_source(&meeting) {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    for name in [
        Channel::Mic.wav_filename(),
        Channel::System.wav_filename(),
        layout::SEGMENTS_FILE,
    ] {
        let from = source.join(name);
        if from.is_file()
            && let Err(error) = std::fs::copy(&from, scratch.join("audio").join(name))
        {
            eprintln!("could not copy {}: {error}", from.display());
            return ExitCode::FAILURE;
        }
    }

    let environment = registry::Environment::discover(&locale, &model_id);
    eprintln!(
        "environment: sidecar={:?} whisper_model={:?}",
        environment.sidecar, environment.whisper_model
    );

    let (selection, mut engine) = match registry::select(preference, &environment) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("engine unavailable: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "selected {} ({})",
        selection.engine.name(),
        selection.reason
    );

    let paths = stt::MeetingPaths::new(&scratch);
    match stt::transcribe_meeting(&paths, engine.as_mut()) {
        Ok(outcome) => {
            eprintln!("{} lines via {}", outcome.lines, outcome.engine);
            println!("{}", outcome.transcript_path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("transcription failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        std::fs::write(path, b"").expect("write");
    }

    #[test]
    fn flat_folder_is_used_as_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("mic.wav"));
        assert_eq!(audio_source(dir.path()), Ok(dir.path().to_path_buf()));
    }

    #[test]
    fn audio_subfolder_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("mic.wav"));
        touch(&layout::audio_dir(dir.path()).join("mic.wav"));
        assert_eq!(audio_source(dir.path()), Ok(layout::audio_dir(dir.path())));
    }

    #[test]
    fn empty_folder_is_an_error_naming_both_places() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = audio_source(dir.path()).expect_err("no audio");
        assert!(error.starts_with("no mic.wav in "), "{error}");
        assert!(error.contains(&dir.path().display().to_string()));
        assert!(error.contains("audio"));
    }
}
