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
//! On macOS, the main loop also polls `kAudioHardwarePropertyDefaultOutputDevice`
//! and `...DefaultInputDevice` ([`audio::macos::device_watch`]) and, on a
//! change, closes the current segment and reopens a new one
//! ([`reopen_segment`]) — SPEC §5's AirPods connect/disconnect exit-gate
//! condition. Both channels are rebuilt on any single device change, even
//! though only one of them physically needs it: `segments.json`'s frame
//! counts are segment-relative (`audio::segments`), so the channel that did
//! not change still needs its counter reset to 0 for that to hold. The one
//! `AudioSource` boundary the format still has no reopen path for is
//! sleep/wake (`reason::SYSTEM_WAKE`) — that needs a different detection
//! mechanism (an `IOKit`/`NSWorkspace` sleep notification, not a polled
//! property) and is not wired up here.

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

/// Contract §6's head-pad: measure each channel's first resampled buffer,
/// take the earlier one as the segment's `start_host_ns`, and pad whichever
/// channel came up later with that much silence so frame 0 of both channels
/// lands on the same instant.
///
/// Factored out of `record()` so both the recording's first segment and
/// every later reopen ([`reopen_segment`]) go through the identical
/// alignment logic instead of two copies that could drift apart — exactly
/// the kind of duplication contract revision 3 above exists to avoid.
fn align_and_pad(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
) -> Result<u64, String> {
    let mic_first = wait_first_position(mic, FIRST_BUFFER_TIMEOUT)
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
    if let (Some(source), Some((sys_ns, _))) = (sys.as_deref_mut(), sys_first)
        && sys_ns > start_host_ns
    {
        let pad = pad_frames_for_gap(sys_ns - start_host_ns);
        println!("meet-rec: padding system-audio head with {pad} frames of silence");
        source
            .pad_leading_silence(pad)
            .map_err(|e| format!("padding system-audio head: {e}"))?;
    }

    Ok(start_host_ns)
}

/// Close the current segment and open a new one, rebuilding whichever
/// channel(s) need a fresh OS-level stream after a default-device change
/// (contract §5/§11's F1; SPEC §5's AirPods-swap gate).
///
/// Always rebuilds *both* channels — see this file's module docs for why a
/// spurious restart on the unaffected channel is the right trade against the
/// alternative (a per-channel segment-relative baseline offset).
///
/// Stops both channels *first*, then reads their final position — never the
/// other order. `stop()` halts the capture stream and joins its worker
/// thread before its own internal `fsync_data`/`patch_header`, so once it
/// returns, `position()` and the just-patched header are guaranteed to agree
/// exactly. Reading `position()` first and calling `stop()` after would
/// leave a window where the (still-running) worker thread appends more audio
/// that `stop()`'s internal fsync then picks up — so the header would end up
/// declaring more frames than the close anchor this function commits to
/// `segments.json`, reproducing the exact header-ahead-of-segments bug
/// `WavWriter::patch_header`'s `synced_frames` freeze was built to prevent
/// one layer down (TUR-54; `crates/audio/src/wav_writer.rs`). Caught by
/// running this function against real hardware and checking `drift-check`'s
/// own invariant check, not by inspection.
#[allow(clippy::too_many_arguments)]
fn reopen_segment(
    mic: &mut Box<dyn AudioSource>,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    segments_path: &Path,
    mic_path: &Path,
    sys_path: &Path,
    reason: &str,
) -> Result<(), String> {
    mic.stop()
        .map_err(|e| format!("stopping microphone for reopen: {e}"))?;
    if let Some(s) = sys.as_mut() {
        s.stop()
            .map_err(|e| format!("stopping system audio for reopen: {e}"))?;
    }

    let mic_close = mic
        .position()
        .ok_or_else(|| "microphone stopped producing audio before a segment reopen".to_string())?;
    let sys_close = match sys.as_deref() {
        Some(s) => s.position().unwrap_or((0, 0)),
        None => (0, 0),
    };

    let mut new_mic: Box<dyn AudioSource> = Box::new(MicSource::new());
    new_mic
        .start(mic_path.to_path_buf())
        .map_err(|e| format!("restarting microphone after reopen: {e}"))?;

    let mut new_sys: Option<Box<dyn AudioSource>> = if sys.is_some() {
        match make_system_source() {
            Some(mut source) => match source.start(sys_path.to_path_buf()) {
                Ok(()) => Some(source),
                Err(e) => {
                    eprintln!(
                        "meet-rec: system audio unavailable after reopen ({e}); \
                         continuing microphone-only"
                    );
                    None
                }
            },
            None => None,
        }
    } else {
        None
    };

    let new_start_host_ns = align_and_pad(&mut *new_mic, &mut new_sys)?;

    let start_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .ok();
    let next_open = SegmentOpen {
        start_host_ns: new_start_host_ns,
        start_continuous_ns: None,
        start_unix_ns,
        mic_rate: SAMPLE_RATE_HZ,
        sys_rate: if new_sys.is_some() { SAMPLE_RATE_HZ } else { 0 },
        mic_device_rate: None,
        sys_device_rate: None,
        reason: reason.to_string(),
    };

    writer.update_frames(mic_close.1, sys_close.1);
    writer.close_segment(
        Anchor {
            mic_host_ns: mic_close.0,
            mic_frames: mic_close.1,
            sys_host_ns: sys_close.0,
            sys_frames: sys_close.1,
        },
        next_open,
    );
    writer
        .write_atomic(segments_path)
        .map_err(|e| format!("writing segments.json at segment reopen: {e}"))?;

    *mic = new_mic;
    *sys = new_sys;
    Ok(())
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
    let start_host_ns = align_and_pad(&mut *mic, &mut sys)?;

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

    // SPEC §5's AirPods-swap gate: baseline the default devices right after
    // the segment they belong to has already opened, so a swap mid-startup
    // (unlikely, but free to handle correctly) is not mistaken for one that
    // happened during the recording.
    #[cfg(target_os = "macos")]
    let mut last_output_device = audio::macos::device_watch::default_output_device().ok();
    #[cfg(target_os = "macos")]
    let mut last_input_device = audio::macos::device_watch::default_input_device().ok();

    let started = Instant::now();
    let checkpoint_interval = Duration::from_secs(CHECKPOINT_INTERVAL_S);
    let mut last_checkpoint = Instant::now();
    loop {
        std::thread::sleep(POLL_INTERVAL);

        let hit_duration = duration.is_some_and(|d| started.elapsed() >= d);
        let hit_stop = stop.load(Ordering::Acquire);

        #[cfg(target_os = "macos")]
        {
            if let Ok(current) = audio::macos::device_watch::default_output_device() {
                if last_output_device.is_some_and(|prev| prev != current) {
                    println!("meet-rec: default output device changed — reopening segment");
                    reopen_segment(
                        &mut mic,
                        &mut sys,
                        &mut writer,
                        &segments_path,
                        &mic_path,
                        &sys_path,
                        segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
                    )?;
                    last_input_device = audio::macos::device_watch::default_input_device().ok();
                    last_checkpoint = Instant::now();
                }
                last_output_device = Some(current);
            }
            if let Ok(current) = audio::macos::device_watch::default_input_device() {
                if last_input_device.is_some_and(|prev| prev != current) {
                    println!("meet-rec: default input device changed — reopening segment");
                    reopen_segment(
                        &mut mic,
                        &mut sys,
                        &mut writer,
                        &segments_path,
                        &mic_path,
                        &sys_path,
                        segments::reason::DEFAULT_INPUT_DEVICE_CHANGED,
                    )?;
                    last_output_device = audio::macos::device_watch::default_output_device().ok();
                    last_checkpoint = Instant::now();
                }
                last_input_device = Some(current);
            }
        }

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A hardware-free `AudioSource` for exercising [`align_and_pad`]'s
    /// alignment maths, which is the one piece of this file's device-change
    /// handling that does not itself need Core Audio or `cpal` — everything
    /// else in [`reopen_segment`] is OS integration, verified on real
    /// hardware instead (see `crates/audio/tests/*_closed_loop.rs`).
    ///
    /// `padded_frames` is an `Arc` specifically so a test can keep its own
    /// handle to it after the `FakeSource` has been moved into a
    /// `Box<dyn AudioSource>` — taking a raw reference to a field and moving
    /// the struct afterward would leave that reference dangling.
    struct FakeSource {
        channel: Channel,
        position: Option<(u64, u64)>,
        padded_frames: Arc<Mutex<Option<u64>>>,
    }

    impl FakeSource {
        fn new(channel: Channel, position: Option<(u64, u64)>) -> Self {
            Self {
                channel,
                position,
                padded_frames: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl AudioSource for FakeSource {
        fn start(&mut self, _dest: PathBuf) -> Result<(), AudioError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
        fn channel(&self) -> Channel {
            self.channel
        }
        fn position(&self) -> Option<(u64, u64)> {
            self.position
        }
        fn fsync_data(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
        fn patch_header(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
        fn pad_leading_silence(&mut self, frames: u64) -> Result<(), AudioError> {
            *self.padded_frames.lock().unwrap() = Some(frames);
            Ok(())
        }
    }

    #[test]
    fn align_and_pad_pads_whichever_channel_came_up_later() {
        let mut mic = FakeSource::new(Channel::Mic, Some((1_000_000_000, 0)));
        let mic_padded = Arc::clone(&mic.padded_frames);
        let sys_source = FakeSource::new(Channel::System, Some((1_050_000_000, 0)));
        let sys_padded = Arc::clone(&sys_source.padded_frames);
        let mut sys: Option<Box<dyn AudioSource>> = Some(Box::new(sys_source));

        let start = align_and_pad(&mut mic, &mut sys).expect("both channels report a position");

        assert_eq!(
            start, 1_000_000_000,
            "start_host_ns must be the earlier of the two channels"
        );
        assert_eq!(
            *mic_padded.lock().unwrap(),
            None,
            "the channel that came up first is never padded"
        );
        assert_eq!(
            *sys_padded.lock().unwrap(),
            Some(800),
            "the later channel is padded by exactly the gap: 50ms at 16kHz is 800 frames"
        );
    }

    #[test]
    fn align_and_pad_pads_the_mic_when_the_mic_comes_up_later() {
        let mut mic = FakeSource::new(Channel::Mic, Some((1_100_000_000, 0)));
        let mic_padded = Arc::clone(&mic.padded_frames);
        let sys_source = FakeSource::new(Channel::System, Some((1_000_000_000, 0)));
        let sys_padded = Arc::clone(&sys_source.padded_frames);
        let mut sys: Option<Box<dyn AudioSource>> = Some(Box::new(sys_source));

        let start = align_and_pad(&mut mic, &mut sys).unwrap();

        assert_eq!(start, 1_000_000_000);
        assert_eq!(*sys_padded.lock().unwrap(), None);
        assert_eq!(
            *mic_padded.lock().unwrap(),
            Some(1600),
            "100ms at 16kHz is 1600 frames"
        );
    }

    #[test]
    fn align_and_pad_with_no_system_channel_uses_mic_alone() {
        let mut mic = FakeSource::new(Channel::Mic, Some((2_000_000_000, 0)));
        let mic_padded = Arc::clone(&mic.padded_frames);
        let mut sys: Option<Box<dyn AudioSource>> = None;

        let start = align_and_pad(&mut mic, &mut sys).unwrap();

        assert_eq!(start, 2_000_000_000);
        assert_eq!(*mic_padded.lock().unwrap(), None);
    }
}
