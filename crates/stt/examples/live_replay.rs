//! Play a recorded meeting into the live session API and print what the pane
//! would be told, one JSON object per line.
//!
//! This exists so the Phase 2 live transcript pane can be built and demoed
//! before `meet-rec` has a live tap. Both tracks run as two concurrent
//! sessions sharing one `seq` counter and one transcript sink, which is
//! exactly the shape the real recorder will use.
//!
//! ```text
//! just live-replay                       # the 60 s two-speaker fixture, real time
//! just live-replay ARGS="--speed 8"      # same, eight times faster
//! cargo run -p stt --example live_replay -- --meeting ~/Meetings/2026-09-27-standup
//! ```
//!
//! stdout is NDJSON and nothing else, so it can be piped straight into a UI
//! harness. Progress, the settled transcript, and errors go to stderr.
//!
//! Every line is a serialized `LiveUpdate`:
//!
//! ```json
//! {"kind":"volatile","seq":4,"speaker":"you","start_sec":1.6,"text":"Morning — can"}
//! {"kind":"final","seq":9,"speaker":"you","start_sec":1.6,"text":"Morning — can everyone hear me?"}
//! {"kind":"dropped","speaker":"others","seq":11}
//! ```
//!
//! The contract the pane implements: a `volatile` replaces that speaker's live
//! line, a `final` clears it and appends a settled line, a `dropped` clears it
//! and appends nothing. At most one live line per speaker. `seq` is unique
//! across both speakers, so it is a safe React key.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use stt::replay::{ReplayEngine, ReplayOptions, replay_track};
use stt::session::SharedCollector;
use stt::{LiveUpdate, SeqCounter, SessionOptions, Speaker};

const USAGE: &str = "\
usage: cargo run -p stt --example live_replay -- [options]

  --meeting DIR   directory holding mic.wav and system.wav
                  (default: crates/audio/fixtures/two-speaker-60s)
  --speed N       playback rate; 1 = real time, 0 = as fast as possible
  --rate N        volatile updates per second per speaker (default 5)
  --chunk MS      audio handed to the session per feed call (default 100)
";

struct Args {
    meeting: PathBuf,
    replay: ReplayOptions,
    volatile_per_sec: f64,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("live_replay: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        meeting: default_meeting(),
        replay: ReplayOptions::realtime(),
        volatile_per_sec: 5.0,
    };

    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        let mut value = || raw.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--meeting" => args.meeting = PathBuf::from(value()?),
            "--speed" => {
                args.replay.speed = value()?.parse().map_err(|_| "--speed wants a number")?
            }
            "--rate" => {
                args.volatile_per_sec = value()?.parse().map_err(|_| "--rate wants a number")?
            }
            "--chunk" => {
                let ms: u64 = value()?.parse().map_err(|_| "--chunk wants milliseconds")?;
                args.replay.chunk = std::time::Duration::from_millis(ms.max(1));
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other}")),
        }
    }

    Ok(args)
}

fn default_meeting() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../audio/fixtures/two-speaker-60s")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("crates/audio/fixtures/two-speaker-60s"))
}

fn run(args: &Args) -> Result<(), String> {
    let tracks = [
        (Speaker::You, args.meeting.join("mic.wav")),
        (Speaker::Others, args.meeting.join("system.wav")),
    ];
    let present: Vec<_> = tracks
        .into_iter()
        .filter(|(_, wav)| wav.is_file())
        .collect();
    if present.is_empty() {
        return Err(format!(
            "no mic.wav or system.wav under {} — run `just fixtures` first",
            args.meeting.display()
        ));
    }

    // One counter and one sink for the whole meeting, two sessions feeding
    // them. That is the invariant the pane depends on: `seq` never collides
    // across speakers, and both tracks land in the same transcript.
    let seq = SeqCounter::new();
    let transcript = SharedCollector::new();

    eprintln!(
        "replaying {} track(s) from {} at {}× ...",
        present.len(),
        args.meeting.display(),
        if args.replay.speed > 0.0 {
            args.replay.speed.to_string()
        } else {
            "unlimited".to_string()
        }
    );

    let outcomes = std::thread::scope(|scope| {
        let handles: Vec<_> = present
            .into_iter()
            .map(|(speaker, wav)| {
                let options = SessionOptions::new(speaker)
                    .with_seq(seq.clone())
                    .with_volatile_per_sec(args.volatile_per_sec);
                let sink = transcript.clone();
                let replay = args.replay.clone();
                scope.spawn(move || {
                    let mut engine = ReplayEngine::new();
                    replay_track(
                        &wav,
                        &mut engine,
                        options,
                        Box::new(sink),
                        Box::new(emit),
                        &replay,
                    )
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "a replay thread panicked".to_string())?
                    .map_err(|error| error.to_string())
            })
            .collect::<Vec<_>>()
    });

    for outcome in outcomes {
        let outcome = outcome?;
        eprintln!(
            "{:?}: {} finalized line(s) over {} s{}",
            outcome.speaker,
            outcome.finalized,
            outcome.audio_sec,
            if outcome.discarded_volatile {
                ", one unsettled guess discarded"
            } else {
                ""
            }
        );
    }

    eprintln!("\n--- transcript.md as it would have been written ---");
    for line in transcript.lines() {
        eprintln!("{line}");
    }
    Ok(())
}

/// One JSON object per line on stdout, locked so two tracks cannot interleave
/// mid-line.
fn emit(update: &LiveUpdate) {
    let Ok(json) = serde_json::to_string(update) else {
        return;
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}
