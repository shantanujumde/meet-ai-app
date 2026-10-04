//! The Windows system-audio source against whatever audio endpoint the
//! machine has (TUR-37): WASAPI loopback on the default output device, with
//! the silence keepalive, written to a real `system.wav`.
//!
//! Not ignored: on a machine with no audio endpoint (a GitHub Actions
//! Windows runner, usually) it prints why it skipped and passes, so the
//! `rust (windows)` job runs it every time and records which case it hit.
//! With an endpoint, nothing has to play: the keepalive alone must make the
//! loopback deliver, and the track must keep wall time.
//!
//! What this cannot check (drift over 10 minutes against a real microphone,
//! a headset switch, a hard kill) is in `docs/manual-checks/worktree-tur37.md`.

// The `stub-audio` build has no real source to record with.
#![cfg(all(target_os = "windows", not(feature = "stub-audio")))]

use std::io::Write as _;
use std::time::{Duration, Instant};

use audio::session::default_system_source;
use audio::{AudioSource, Channel, Error};

/// How long the test records once the first packet is in.
const RECORD: Duration = Duration::from_secs(3);

/// How long the first packet may take; the keepalive makes it immediate.
const FIRST_PACKET: Duration = Duration::from_secs(5);

/// How far the track's length may be from the wall time it covered: two
/// resampler chunks and the 80 ms device buffer, generously.
const WALL_TIME_SLACK_MS: f64 = 250.0;

/// A line for the CI log even when the test passes: libtest captures
/// `eprintln!`, but not writes to the stderr handle itself.
fn log(line: &str) {
    let _ = writeln!(std::io::stderr(), "windows_loopback: {line}");
}

fn wait_for_position(source: &dyn AudioSource, budget: Duration) -> Option<(u64, u64)> {
    let started = Instant::now();
    while started.elapsed() < budget {
        if let Some(position) = source.position() {
            return Some(position);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn loopback_records_the_default_output_or_skips_without_one() {
    let mut source =
        default_system_source().expect("Windows has a system-audio source since TUR-37");
    assert_eq!(source.channel(), Channel::System);

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("system.wav");
    match source.start(dest.clone()) {
        Ok(()) => {}
        Err(Error::NoDevice(reason)) => {
            log(&format!(
                "SKIPPED: this machine has no usable audio output device ({reason})"
            ));
            return;
        }
        Err(other) => panic!("starting WASAPI loopback failed: {other}"),
    }
    log(&format!(
        "recording loopback: {}",
        source.rate_report().unwrap_or_default()
    ));

    let first = wait_for_position(&*source, FIRST_PACKET);
    let Some((first_ns, first_frames)) = first else {
        source.stop().unwrap();
        panic!(
            "the loopback opened but delivered nothing in {FIRST_PACKET:?}: the silence \
             keepalive did not keep it running"
        );
    };
    std::thread::sleep(RECORD);
    let (last_ns, last_frames) = source.position().expect("still recording");
    source.stop().unwrap();

    assert!(
        last_ns > first_ns,
        "capture times did not move: {first_ns} then {last_ns}"
    );
    let wall_ms = (last_ns - first_ns) as f64 / 1e6;
    let track_ms = (last_frames - first_frames) as f64 / 16.0;
    log(&format!(
        "{track_ms:.0} ms of track for {wall_ms:.0} ms of capture time"
    ));
    assert!(
        (track_ms - wall_ms).abs() < WALL_TIME_SLACK_MS,
        "{track_ms:.0} ms of track for {wall_ms:.0} ms of capture time"
    );

    let header_frames = audio::wav_writer::read_header_frames(&dest).unwrap();
    let (_, final_frames) = source.position().unwrap();
    assert_eq!(
        header_frames, final_frames,
        "header agrees with position after stop"
    );
    assert!(
        header_frames >= 16_000 * 2,
        "{header_frames} frames for 3 s"
    );
}
