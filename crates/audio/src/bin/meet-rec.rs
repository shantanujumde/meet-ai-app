//! `meet-rec` — the standalone capture CLI.
//!
//! SPEC §4: Phase 0 needs no Tauri and no UI. This binary is how the capture
//! work gets built and tested on its own.
//!
//! All the orchestration — both `AudioSource`s (SPEC §5), the head-pad that
//! aligns their frame 0 to a shared `start_host_ns` (contract §6), the
//! checkpoint order that keeps `mic.wav`/`system.wav`/`segments.json`
//! mutually consistent under `kill -9` (contract §7/§11), and the
//! default-device-change segment reopen (SPEC §5's AirPods connect/disconnect
//! exit-gate condition) — lives in [`audio::session::RecordingSession`]
//! (TUR-94). This binary is a thin caller over it: build the real sources,
//! start the session, poll it (`RecordingSession::tick`) until told to stop,
//! then stop it and print what actually landed on disk. The app is meant to
//! be an equally thin caller over the same session, so the two cannot drift
//! apart on what "the real recorder" does.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use audio::AudioSource;
use audio::mic::MicSource;
use audio::segments::{SAMPLE_RATE_HZ, Segments};
use audio::session::{RecordingSession, default_system_source};
use audio::wav_writer::read_header_frames;

const USAGE: &str = "\
meet-rec — record a meeting's microphone and system audio to WAV.

USAGE:
    meet-rec --list-devices
    meet-rec --out <DIR> [--duration <SECS>]

OPTIONS:
    --list-devices     Print the input and output devices meet-ai can see.
    --out <DIR>        Write mic.wav, system.wav and segments.json into DIR.
    --duration <SECS>  Stop automatically after SECS seconds, instead of
                        waiting for Enter. Mainly for scripted verification.
    -h, --help         Print this message.
";

/// How often the main loop wakes to check the stop condition and call
/// [`audio::session::RecordingSession::tick`]. Coarse next to
/// `audio::segments::CHECKPOINT_INTERVAL_S`; only bounds how late a stop
/// request or a checkpoint is noticed.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("-h") | Some("--help") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("--list-devices") => list_devices(),
        Some("--out") => {
            let Some(dir) = args.get(1) else {
                eprintln!("meet-rec: --out requires a directory argument");
                return ExitCode::FAILURE;
            };
            let duration = match parse_duration_flag(&args[2..]) {
                Ok(d) => d,
                Err(msg) => {
                    eprintln!("meet-rec: {msg}");
                    return ExitCode::FAILURE;
                }
            };
            match record(PathBuf::from(dir), duration) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("meet-rec: {msg}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(other) => {
            eprintln!("meet-rec: `{other}` is not a recognized option.");
            eprintln!();
            print!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn parse_duration_flag(rest: &[String]) -> Result<Option<Duration>, String> {
    match rest.first().map(String::as_str) {
        None => Ok(None),
        Some("--duration") => {
            let secs: u64 = rest
                .get(1)
                .ok_or_else(|| "--duration requires a number of seconds".to_string())?
                .parse()
                .map_err(|_| "--duration must be a whole number of seconds".to_string())?;
            Ok(Some(Duration::from_secs(secs)))
        }
        Some(other) => Err(format!("`{other}` is not a recognized option")),
    }
}

fn list_devices() -> ExitCode {
    use cpal::traits::HostTrait;

    let host = cpal::default_host();
    println!("Input devices:");
    match host.input_devices() {
        Ok(devices) => {
            for device in devices {
                println!("  {device}");
            }
        }
        Err(e) => println!("  (could not enumerate: {e})"),
    }

    println!("Output devices:");
    match host.output_devices() {
        Ok(devices) => {
            for device in devices {
                println!("  {device}");
            }
        }
        Err(e) => println!("  (could not enumerate: {e})"),
    }

    ExitCode::SUCCESS
}

/// A thin caller over [`audio::session::RecordingSession`]: build the real
/// sources, start the session, drive its poll loop until told to stop, then
/// stop it and report what actually landed on disk. All the orchestration —
/// head-pad alignment, device-change reopen, checkpointing, the clean-stop
/// order — lives in the session now (TUR-94), so this function and a future
/// in-app caller cannot drift apart on what "the real recorder" does.
fn record(dir: PathBuf, duration: Option<Duration>) -> Result<(), String> {
    let mic: Box<dyn AudioSource> = Box::new(MicSource::new());
    let sys = default_system_source();
    let mut session = RecordingSession::start(dir, mic, sys)?;

    let stop = Arc::new(AtomicBool::new(false));
    match duration {
        None => {
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("meet-rec-stop-watcher".to_string())
                .spawn(move || {
                    println!("meet-rec: recording — press Enter to stop.");
                    let mut line = String::new();
                    let _ = io::stdin().lock().read_line(&mut line);
                    stop.store(true, Ordering::Release);
                })
                .expect("spawning the stop-watcher thread");
        }
        Some(d) => println!("meet-rec: recording for {:.0}s…", d.as_secs_f64()),
    }

    let started = Instant::now();
    loop {
        std::thread::sleep(POLL_INTERVAL);
        session.tick()?;

        let hit_duration = duration.is_some_and(|d| started.elapsed() >= d);
        let hit_stop = stop.load(Ordering::Acquire);
        if hit_duration || hit_stop {
            break;
        }
    }

    println!("meet-rec: stopping…");
    let report = session.stop()?;
    report_result(
        &report.mic_path,
        &report.sys_path,
        &report.segments_path,
        report.has_system_audio,
    );
    Ok(())
}

/// Print what actually landed on disk — frame counts read back from the WAV
/// headers themselves, and the drift measurement if it's computable — rather
/// than assuming success from `record`'s own return value. Per the role
/// charter: report what the bytes contain, not what the API implied.
fn report_result(mic_path: &Path, sys_path: &Path, segments_path: &Path, has_system: bool) {
    let mic_frames = read_header_frames(mic_path);
    println!(
        "meet-rec: {} — {}",
        mic_path.display(),
        match &mic_frames {
            Ok(f) => format!("{f} frames ({:.1}s)", *f as f64 / SAMPLE_RATE_HZ as f64),
            Err(e) => format!("could not read header back: {e}"),
        }
    );
    if has_system {
        let sys_frames = read_header_frames(sys_path);
        println!(
            "meet-rec: {} — {}",
            sys_path.display(),
            match &sys_frames {
                Ok(f) => format!("{f} frames ({:.1}s)", *f as f64 / SAMPLE_RATE_HZ as f64),
                Err(e) => format!("could not read header back: {e}"),
            }
        );
    } else {
        println!(
            "meet-rec: {} — not written (no system-audio channel)",
            sys_path.display()
        );
    }

    match std::fs::read_to_string(segments_path)
        .ok()
        .and_then(|json| Segments::from_json(&json).ok())
    {
        Some(segments) => match segments.drift() {
            Ok(report) => println!(
                "meet-rec: drift — mic {:.1}ms, system {:.1}ms, worst {:.1}ms",
                report.mic.final_ms,
                report.system.final_ms,
                report.worst_ms()
            ),
            Err(e) => println!("meet-rec: drift not measurable: {e}"),
        },
        None => println!("meet-rec: could not re-read segments.json to report drift"),
    }
}
