//! The quit path's wait (TUR-160): on the real recorder's lock, and end to
//! end with a fake recorder that writes a meeting folder the way the real one
//! does, so a quit in each phase can be checked for a finished folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use super::*;

/// Short bounds, so a test that hits one is quick.
const TEST_BOUNDS: Bounds = Bounds {
    starting: Duration::from_secs(10),
    stopping: Duration::from_secs(10),
};

/// How long the fake's start and stop take: long enough that a quit which
/// did not wait would see the folder half done.
const STEP: Duration = Duration::from_millis(150);

#[test]
fn the_quit_bounds_are_the_ones_decided() {
    assert_eq!(QUIT_BOUNDS.starting, Duration::from_secs(15));
    assert_eq!(QUIT_BOUNDS.stopping, STOP_TIMEOUT + Duration::from_secs(5));
}

/// The recorder's own lock wakes a waiter on every change, so the wait ends
/// when the phase moves, not at its bound.
#[test]
fn the_real_recorder_wakes_a_waiter_when_its_phase_moves() {
    let recorder = Recorder::default();
    recorder.lock().status.phase = Phase::Starting;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(STEP);
            recorder.lock().status.phase = Phase::Recording;
        });
        let started = Instant::now();
        let after = recorder.wait_while_in(Phase::Starting, Duration::from_secs(30));
        assert_eq!(after, Phase::Recording);
        assert!(started.elapsed() < Duration::from_secs(10));
    });
}

#[test]
fn the_real_recorder_gives_up_at_the_bound() {
    let recorder = Recorder::default();
    recorder.lock().status.phase = Phase::Stopping;
    let started = Instant::now();
    let after = recorder.wait_while_in(Phase::Stopping, Duration::from_millis(50));
    assert_eq!(after, Phase::Stopping);
    assert!(started.elapsed() >= Duration::from_millis(50));
}

// --- a fake recorder --------------------------------------------------------

/// What the fake writes, in order. A start opens the folder with empty
/// audio; the audio lands once the channels are open (`Recording`); a stop
/// writes the last of it, `segments.json` and the transcript's tail.
struct FakeRecorder {
    state: Watched<Phase>,
    dir: PathBuf,
    stops: AtomicUsize,
}

impl FakeRecorder {
    fn new(dir: &Path) -> Self {
        Self {
            state: Watched::new(Phase::Idle),
            dir: dir.to_path_buf(),
            stops: AtomicUsize::new(0),
        }
    }

    fn set(&self, phase: Phase) {
        *self.state.lock() = phase;
    }

    /// The start, as `Recorder::start` runs it on its own thread: the folder
    /// first, with nothing in it, then `Recording` once the audio flows.
    fn start(&self) {
        self.set(Phase::Starting);
        std::fs::create_dir_all(self.dir.join("audio")).unwrap();
        std::fs::write(self.dir.join("audio").join("mic.wav"), b"").unwrap();
        std::fs::write(self.dir.join("transcript.md"), "").unwrap();
        std::thread::sleep(STEP);
        std::fs::write(self.dir.join("audio").join("mic.wav"), b"RIFF-start").unwrap();
        self.set(Phase::Recording);
    }

    /// Closing the files, as `stop::close` does once `Stopping` is claimed.
    fn close(&self) {
        std::thread::sleep(STEP);
        std::fs::write(self.dir.join("audio").join("mic.wav"), b"RIFF-whole").unwrap();
        std::fs::write(self.dir.join("segments.json"), "{}").unwrap();
        std::fs::write(self.dir.join("transcript.md"), "[00:00:01] You: Bye.\n").unwrap();
        self.set(Phase::Idle);
    }

    /// Whether the folder is the one a clean stop leaves.
    fn folder_is_finished(&self) -> bool {
        let read = |path: PathBuf| std::fs::read(path).unwrap_or_default();
        read(self.dir.join("audio").join("mic.wav")) == b"RIFF-whole"
            && self.dir.join("segments.json").is_file()
            && read(self.dir.join("transcript.md")).ends_with(b"Bye.\n")
    }
}

impl Settle for FakeRecorder {
    fn phase(&self) -> Phase {
        *self.state.lock()
    }

    fn wait_while_in(&self, phase: Phase, bound: Duration) -> Phase {
        *self.state.wait_while(bound, |now| *now == phase)
    }

    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        {
            let mut phase = self.state.lock();
            if *phase != Phase::Recording {
                return;
            }
            *phase = Phase::Stopping;
        }
        self.close();
    }
}

/// Wait (briefly) until `fake` is in `phase`, so the quit lands mid-way.
fn wait_for(fake: &FakeRecorder, phase: Phase) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while fake.phase() != phase {
        assert!(
            Instant::now() < deadline,
            "the fake never reached {phase:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn quitting_while_starting_waits_for_the_start_then_stops_it() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeRecorder::new(&tmp.path().join("2026-10-10-1500-meeting"));
    std::thread::scope(|scope| {
        scope.spawn(|| fake.start());
        wait_for(&fake, Phase::Starting);
        assert_eq!(finish(&fake, TEST_BOUNDS), Phase::Idle);
    });
    assert!(fake.folder_is_finished(), "the quit cut the recording");
    assert_eq!(fake.stops.load(Ordering::SeqCst), 1);
}

#[test]
fn quitting_while_stopping_waits_for_that_stop_to_finish() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeRecorder::new(&tmp.path().join("2026-10-10-1500-meeting"));
    fake.start();
    std::thread::scope(|scope| {
        // The user's Stop, on a thread of its own, is still closing the files.
        scope.spawn(|| fake.stop());
        wait_for(&fake, Phase::Stopping);
        assert_eq!(finish(&fake, TEST_BOUNDS), Phase::Idle);
        assert!(
            fake.folder_is_finished(),
            "the quit did not wait for the stop"
        );
    });
    // Only the user's Stop: the quit had nothing left to stop.
    assert_eq!(fake.stops.load(Ordering::SeqCst), 1);
}

#[test]
fn quitting_while_recording_stops_it() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeRecorder::new(&tmp.path().join("2026-10-10-1500-meeting"));
    fake.start();
    assert_eq!(finish(&fake, TEST_BOUNDS), Phase::Idle);
    assert!(fake.folder_is_finished());
}

#[test]
fn quitting_while_idle_does_nothing_and_does_not_wait() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeRecorder::new(&tmp.path().join("2026-10-10-1500-meeting"));
    let started = Instant::now();
    assert_eq!(finish(&fake, TEST_BOUNDS), Phase::Idle);
    assert!(started.elapsed() < STEP);
    assert!(!fake.dir.exists());
}

/// A start that never settles holds the quit only up to its bound.
#[test]
fn a_start_that_never_settles_holds_the_quit_only_up_to_its_bound() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = FakeRecorder::new(&tmp.path().join("2026-10-10-1500-meeting"));
    fake.set(Phase::Starting);
    let bounds = Bounds {
        starting: Duration::from_millis(50),
        stopping: Duration::from_millis(50),
    };
    let started = Instant::now();
    assert_eq!(finish(&fake, bounds), Phase::Starting);
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(50), "{waited:?}");
    assert!(waited < Duration::from_secs(5), "{waited:?}");
    // Stopped anyway, which a start in progress turns away.
    assert_eq!(fake.stops.load(Ordering::SeqCst), 1);
}
