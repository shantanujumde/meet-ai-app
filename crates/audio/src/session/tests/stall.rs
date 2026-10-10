//! TUR-163 at the session level: a channel whose position stops moving is
//! noticed at the next checkpoint, and a stream that says it died at the
//! next tick; either way the segment is reopened on fresh sources.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::*;

/// A started session over stub sources, with handles on the first ones.
struct Rig {
    _tmp: tempfile::TempDir,
    session: RecordingSession,
    mic_behind: Arc<AtomicU64>,
    sys_behind: Arc<AtomicU64>,
    mic_lost: Arc<AtomicBool>,
}

impl Rig {
    fn start() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let mic = StubSource::new(Channel::Mic);
        let sys = StubSource::new(Channel::System);
        let (mic_behind, sys_behind) = (Arc::clone(&mic.behind_ns), Arc::clone(&sys.behind_ns));
        let mic_lost = Arc::clone(&mic.lost);
        let session =
            RecordingSession::start(tmp.path().to_path_buf(), Box::new(mic), Some(Box::new(sys)))
                .expect("session starts");
        Self {
            _tmp: tmp,
            session,
            mic_behind,
            sys_behind,
            mic_lost,
        }
    }

    /// One tick, rebuilding any reopened source as a fresh stub.
    fn tick(&mut self) -> Result<(), String> {
        self.session
            .tick_with(&|| stub(Channel::Mic), &|| Some(stub(Channel::System)))
    }

    fn make_checkpoint_due(&mut self) {
        self.session.last_checkpoint = Instant::now()
            .checked_sub(Duration::from_secs(CHECKPOINT_INTERVAL_S))
            .expect("the monotonic clock is past one checkpoint interval");
    }

    fn reasons(&self) -> Vec<String> {
        read_segments(&self.session.segments_path)
            .segments
            .iter()
            .map(|segment| segment.reason.clone())
            .collect()
    }
}

const STALLED: u64 = 3_000_000_000;

/// The "Done when": a microphone whose stream stopped is noticed at the
/// next checkpoint, which reopens the segment instead of passing.
#[test]
fn a_stalled_microphone_reopens_the_segment_at_the_next_checkpoint() {
    let mut rig = Rig::start();
    rig.mic_behind.store(STALLED, Ordering::SeqCst);
    rig.make_checkpoint_due();
    rig.tick().expect("a stall is not an error");
    assert_eq!(
        rig.reasons(),
        vec![
            segments::reason::START.to_string(),
            segments::reason::STREAM_RESTART.to_string()
        ]
    );
    assert!(rig.session.status().has_system_audio, "both rebuilt");

    // The fresh microphone keeps writing: the next checkpoint passes.
    rig.make_checkpoint_due();
    rig.tick().unwrap();
    assert_eq!(rig.reasons().len(), 2);
    rig.session.stop().expect("stops cleanly");
}

#[test]
fn a_stalled_system_track_reopens_the_segment_too() {
    let mut rig = Rig::start();
    rig.sys_behind.store(STALLED, Ordering::SeqCst);
    rig.make_checkpoint_due();
    rig.tick().unwrap();
    assert_eq!(
        rig.reasons().last().map(String::as_str),
        Some(segments::reason::STREAM_RESTART)
    );
}

#[test]
fn channels_that_keep_writing_are_left_alone() {
    let mut rig = Rig::start();
    rig.make_checkpoint_due();
    rig.tick().unwrap();
    assert_eq!(rig.reasons(), vec![segments::reason::START.to_string()]);
}

/// The lid-closed case: the microphone the recording opened went away
/// while the default input stayed the same, and its stream said so.
#[test]
fn a_lost_microphone_stream_reopens_at_the_next_tick() {
    let mut rig = Rig::start();
    rig.mic_lost.store(true, Ordering::SeqCst);
    rig.tick().unwrap();
    assert_eq!(
        rig.reasons(),
        vec![
            segments::reason::START.to_string(),
            segments::reason::STREAM_RESTART.to_string()
        ]
    );
    // The fresh microphone's stream is fine: no second reopen.
    rig.tick().unwrap();
    assert_eq!(rig.reasons().len(), 2);
}
