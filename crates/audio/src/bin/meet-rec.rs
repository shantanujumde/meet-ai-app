//! `meet-rec` — the standalone capture CLI.
//!
//! SPEC §4: Phase 0 needs no Tauri and no UI. This binary is how the capture
//! work gets built and tested on its own.
//!
//! Orchestrates both `AudioSource`s (SPEC §5), the head-pad that aligns their
//! frame 0 to a shared `start_host_ns` (contract §6), and the checkpoint
//! order that keeps `mic.wav`/`system.wav`/`segments.json` mutually
//! consistent under `kill -9` (contract §7/§11): fsync every channel's data,
//! write `segments.json`, then patch every channel's header — never the
//! other order.
//!
//! Honest gap: this binary writes exactly one segment per recording. Device
//! changes (SPEC §5's AirPods swap) and sleep/wake are supposed to close and
//! reopen a segment (contract §5/§11's F1) — that reopening is not wired up
//! yet, so today's `meet-rec` cannot itself demonstrate the AirPods-survival
//! exit-gate condition. Everything the *format* needs for that (`close_segment`,
//! close anchors, `start_continuous_ns`) already exists in
//! [`audio::segments`]; what is missing is the device-change detection that
//! would call it.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use audio::mic::MicSource;
use audio::segments::{
    self, Anchor, CHECKPOINT_INTERVAL_S, SAMPLE_RATE_HZ, SegmentOpen, Segments, SegmentsWriter,
};
use audio::wav_writer::read_header_frames;
use audio::{AudioSource, Channel, Error as AudioError};

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

/// How long to wait for each channel's very first resampled buffer before
/// giving up on head-pad alignment (contract §6) and reporting that channel
/// as broken rather than hanging. Generous relative to the ~50ms warm-path
/// setup Tess measured (TUR-4), because a cold run — first launch after a
/// permission grant — can still show that dialog's latency on top of it.
const FIRST_BUFFER_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the main loop wakes to check the stop condition. Coarse next to
/// [`CHECKPOINT_INTERVAL_S`]; only bounds how late a stop request is noticed.
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

/// The system-audio `AudioSource`, where one exists. `None` on any platform
/// without a process-tap implementation yet (SPEC §8.2's Windows stub) —
/// contract §9's "absent track" path, not an error.
fn make_system_source() -> Option<Box<dyn AudioSource>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(audio::macos::tap::SystemSource::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Poll `source.position()` until it reports its first resampled buffer, or
/// give up after `budget`. Busy-polls at 1ms rather than sleeping longer,
/// because the whole point is to catch the *first* buffer as close to its
/// arrival as this process can — sleeping coarsely here would reintroduce the
/// same flush-latency error contract §11 rejects for anchors.
fn wait_first_position(source: &dyn AudioSource, budget: Duration) -> Result<(u64, u64), String> {
    let started = Instant::now();
    loop {
        if let Some(pos) = source.position() {
            return Ok(pos);
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "no audio arrived within {:.1}s of starting capture",
                budget.as_secs_f64()
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn pad_frames_for_gap(gap_ns: u64) -> u64 {
    ((gap_ns as f64 / 1e9) * SAMPLE_RATE_HZ as f64).round() as u64
}

/// §7/§11's checkpoint order for one mid-recording checkpoint: fsync every
/// channel's data, then let the caller write `segments.json`, then patch
/// every channel's header. This function does steps 1 and 3 and the anchor
/// bookkeeping; the caller supplies the write in between.
fn checkpoint(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    segments_path: &Path,
) -> Result<(), String> {
    mic.fsync_data().map_err(|e| format!("mic fsync: {e}"))?;
    if let Some(sys) = sys.as_deref_mut() {
        sys.fsync_data().map_err(|e| format!("system fsync: {e}"))?;
    }

    let (mic_host_ns, mic_frames) = mic
        .position()
        .ok_or_else(|| "microphone stopped producing audio mid-recording".to_string())?;
    let (sys_host_ns, sys_frames) = match sys.as_deref() {
        Some(sys) => sys.position().unwrap_or((0, 0)),
        None => (0, 0),
    };

    writer.update_frames(mic_frames, sys_frames);
    writer.checkpoint_anchor(Anchor {
        mic_host_ns,
        mic_frames,
        sys_host_ns,
        sys_frames,
    });
    writer
        .write_atomic(segments_path)
        .map_err(|e| format!("writing segments.json: {e}"))?;

    mic.patch_header()
        .map_err(|e| format!("mic header patch: {e}"))?;
    if let Some(sys) = sys.as_deref_mut() {
        sys.patch_header()
            .map_err(|e| format!("system header patch: {e}"))?;
    }
    Ok(())
}

fn record(dir: PathBuf, duration: Option<Duration>) -> Result<(), String> {
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let mic_path = dir.join(Channel::Mic.wav_filename());
    let sys_path = dir.join(Channel::System.wav_filename());
    let segments_path = dir.join("segments.json");

    let mut mic: Box<dyn AudioSource> = Box::new(MicSource::new());
    println!("meet-rec: starting microphone capture…");
    mic.start(mic_path.clone()).map_err(microphone_error)?;

    let mut sys: Option<Box<dyn AudioSource>> = make_system_source();
    if let Some(source) = sys.as_mut() {
        println!("meet-rec: starting system-audio capture…");
        if let Err(e) = source.start(sys_path.clone()) {
            eprintln!(
                "meet-rec: system audio unavailable ({e}); recording microphone only \
                 (contract §9: absent track, sys_rate 0 in segments.json)"
            );
            sys = None;
        }
    } else {
        eprintln!(
            "meet-rec: no system-audio capture on this platform yet (SPEC §8.2); \
             recording microphone only"
        );
    }

    // Head-pad (contract §6): align frame 0 of both channels to whichever
    // channel's hardware came up first, by padding the other with silence.
    println!("meet-rec: measuring channel start alignment…");
    let mic_first = wait_first_position(&*mic, FIRST_BUFFER_TIMEOUT)
        .map_err(|e| format!("microphone produced no audio: {e}"))?;
    let sys_first = match sys.as_deref() {
        Some(source) => Some(
            wait_first_position(source, FIRST_BUFFER_TIMEOUT)
                .map_err(|e| format!("system audio produced no audio: {e}"))?,
        ),
        None => None,
    };

    let start_host_ns = match sys_first {
        Some((sys_ns, _)) => mic_first.0.min(sys_ns),
        None => mic_first.0,
    };

    if mic_first.0 > start_host_ns {
        let pad = pad_frames_for_gap(mic_first.0 - start_host_ns);
        println!("meet-rec: padding microphone head with {pad} frames of silence");
        mic.pad_leading_silence(pad)
            .map_err(|e| format!("padding microphone head: {e}"))?;
    }
    if let (Some(source), Some((sys_ns, _))) = (sys.as_mut(), sys_first)
        && sys_ns > start_host_ns
    {
        let pad = pad_frames_for_gap(sys_ns - start_host_ns);
        println!("meet-rec: padding system-audio head with {pad} frames of silence");
        source
            .pad_leading_silence(pad)
            .map_err(|e| format!("padding system-audio head: {e}"))?;
    }

    let start_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .ok();

    let open = SegmentOpen {
        start_host_ns,
        start_continuous_ns: None,
        start_unix_ns,
        mic_rate: SAMPLE_RATE_HZ,
        sys_rate: if sys.is_some() { SAMPLE_RATE_HZ } else { 0 },
        mic_device_rate: None,
        sys_device_rate: None,
        reason: segments::reason::START.to_string(),
    };
    let mut writer = SegmentsWriter::new(open);
    // First segments.json write happens at the first checkpoint below, not
    // here — nothing has been fsynced yet for it to honestly describe.

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
    let checkpoint_interval = Duration::from_secs(CHECKPOINT_INTERVAL_S);
    let mut last_checkpoint = Instant::now();
    loop {
        std::thread::sleep(POLL_INTERVAL);

        let hit_duration = duration.is_some_and(|d| started.elapsed() >= d);
        let hit_stop = stop.load(Ordering::Acquire);

        if last_checkpoint.elapsed() >= checkpoint_interval {
            checkpoint(&mut *mic, &mut sys, &mut writer, &segments_path)?;
            last_checkpoint = Instant::now();
            println!(
                "meet-rec: checkpoint at {:.0}s",
                started.elapsed().as_secs_f64()
            );
        }

        if hit_duration || hit_stop {
            break;
        }
    }

    println!("meet-rec: stopping…");
    mic.stop()
        .map_err(|e| format!("stopping microphone: {e}"))?;
    if let Some(source) = sys.as_mut() {
        source
            .stop()
            .map_err(|e| format!("stopping system audio: {e}"))?;
    }

    // Final snapshot, taken only after both streams are fully stopped so it
    // is exact — contract §7's "equality on graceful stop" — rather than the
    // periodic checkpoint's necessarily-slightly-behind approximation.
    let (mic_ns, mic_frames) = mic.position().unwrap_or((start_host_ns, 0));
    let (sys_ns, sys_frames) = sys
        .as_deref()
        .and_then(|source| source.position())
        .unwrap_or((0, 0));
    writer.update_frames(mic_frames, sys_frames);
    writer.checkpoint_anchor(Anchor {
        mic_host_ns: mic_ns,
        mic_frames,
        sys_host_ns: sys_ns,
        sys_frames,
    });
    writer
        .write_atomic(&segments_path)
        .map_err(|e| format!("writing segments.json: {e}"))?;

    report_result(&mic_path, &sys_path, &segments_path, sys.is_some());
    Ok(())
}

fn microphone_error(e: AudioError) -> String {
    match e {
        AudioError::PermissionDenied => {
            "meet-ai does not have permission to record the microphone — grant it in \
             System Settings > Privacy & Security > Microphone"
                .to_string()
        }
        other => format!("microphone: {other}"),
    }
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
