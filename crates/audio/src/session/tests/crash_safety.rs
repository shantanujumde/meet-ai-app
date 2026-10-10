//! TUR-162 at the session level.
//!
//! A `kill -9` can land between any two writes, so the files on disk after
//! every step of a stop, a pause, a reopen or a checkpoint must already be
//! readable: no WAV header may declare a frame `segments.json` does not
//! cover (§7). [`Checked`] wraps the stub source and checks exactly that
//! before and after every call the session makes into it. The disk only
//! changes inside those calls and in the session's own `segments.json`
//! writes between them, so every state a kill could leave is checked.
//!
//! Like a live source, it keeps writing: every fsync, a stop's included,
//! first appends another buffer, so there are always frames past the last
//! checkpoint for a header to get ahead with. It also fails `fsync` on
//! demand, for the checkpoint retries.

use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// Every state seen in which a header was ahead of `segments.json`.
type Violations = Arc<Mutex<Vec<String>>>;

/// What a `kill -9` right now would leave: each header against
/// `segments.json` (none on disk covers nothing).
fn check_disk(dir: &Path, at: &str, violations: &Violations) {
    let segments = std::fs::read_to_string(dir.join(meeting_format::layout::SEGMENTS_FILE))
        .ok()
        .map(|json| {
            crate::segments::Segments::from_json(&json).expect("segments.json is never torn")
        });
    for channel in [Channel::Mic, Channel::System] {
        let Ok(header) = crate::wav_writer::read_header_frames(&dir.join(channel.wav_filename()))
        else {
            continue;
        };
        let problem = match &segments {
            Some(s) => s
                .check_wav_header(channel, header)
                .err()
                .map(|e| format!("{e:?}")),
            None if header > 0 => Some(format!("{header} frames and no segments.json")),
            None => None,
        };
        if let Some(problem) = problem {
            violations
                .lock()
                .unwrap()
                .push(format!("{at}: {channel:?} header: {problem}"));
        }
    }
}

/// A [`StubSource`] that checks the disk around every call, writes on
/// until it is stopped, and fails the next `fail_fsyncs` fsyncs (a
/// checkpoint's, or a stop's).
struct Checked {
    inner: StubSource,
    dir: PathBuf,
    violations: Violations,
    fail_fsyncs: Arc<AtomicU32>,
    /// Between a successful start and the stop.
    capturing: bool,
}

impl Checked {
    /// Run `call`, checking the disk just before and just after it.
    fn around<T>(&mut self, call: &str, run: impl FnOnce(&mut Self) -> T) -> T {
        let label = format!("{:?} {call}", self.inner.channel);
        let (dir, violations) = (self.dir.clone(), Arc::clone(&self.violations));
        check_disk(&dir, &format!("before {label}"), &violations);
        let out = run(self);
        check_disk(&dir, &format!("after {label}"), &violations);
        out
    }

    fn take_failure(&self) -> Result<(), AudioError> {
        let failing = self
            .fail_fsyncs
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failing {
            return Err(AudioError::Io(std::io::Error::other(
                "injected fsync failure",
            )));
        }
        Ok(())
    }

    /// What the worker wrote since the last call.
    fn grow(&mut self) -> Result<(), AudioError> {
        if self.capturing {
            self.inner.append_buffer()?;
        }
        Ok(())
    }
}

impl AudioSource for Checked {
    fn start(&mut self, dest: PathBuf) -> Result<(), AudioError> {
        self.around("start", |s| {
            let out = s.inner.start(dest);
            s.capturing = out.is_ok();
            out
        })
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.stop_capture()?;
        self.patch_header()
    }

    fn stop_capture(&mut self) -> Result<(), AudioError> {
        self.around("stop_capture", |s| {
            s.take_failure()?;
            s.grow()?;
            s.capturing = false;
            s.inner.stop_capture()
        })
    }

    fn channel(&self) -> Channel {
        self.inner.channel
    }

    fn position(&self) -> Option<(u64, u64)> {
        let at = format!("{:?} position", self.inner.channel);
        check_disk(&self.dir, &at, &self.violations);
        self.inner.position()
    }

    fn fsync_data(&mut self) -> Result<(), AudioError> {
        self.around("fsync_data", |s| {
            s.take_failure()?;
            s.grow()?;
            s.inner.fsync_data()
        })
    }

    fn patch_header(&mut self) -> Result<(), AudioError> {
        self.around("patch_header", |s| s.inner.patch_header())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), AudioError> {
        self.around("pad", |s| s.inner.pad_leading_silence(frames))
    }

    fn device_rate(&self) -> Option<u32> {
        self.inner.device_rate()
    }
}

/// One recording folder, the violations every [`Checked`] in it reports,
/// and a way to make the next fsyncs fail.
struct Rig {
    tmp: tempfile::TempDir,
    violations: Violations,
    fail_fsyncs: Arc<AtomicU32>,
}

impl Rig {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
            violations: Violations::default(),
            fail_fsyncs: Arc::default(),
        }
    }

    fn dir(&self) -> PathBuf {
        self.tmp.path().to_path_buf()
    }

    fn source(&self, channel: Channel) -> Box<dyn AudioSource> {
        self.source_failing(channel, Arc::default())
    }

    /// A source whose fsyncs fail while `fail_fsyncs` is above zero.
    fn source_failing(
        &self,
        channel: Channel,
        fail_fsyncs: Arc<AtomicU32>,
    ) -> Box<dyn AudioSource> {
        Box::new(Checked {
            inner: StubSource::new(channel),
            dir: self.dir(),
            violations: Arc::clone(&self.violations),
            fail_fsyncs,
            capturing: false,
        })
    }

    fn start(&self) -> RecordingSession {
        let mic = self.source_failing(Channel::Mic, Arc::clone(&self.fail_fsyncs));
        RecordingSession::start(self.dir(), mic, Some(self.source(Channel::System)))
            .expect("session starts cleanly")
    }

    /// The test's last word: no state ever had a header ahead.
    fn assert_never_ahead(&self) {
        let violations = self.violations.lock().unwrap();
        assert!(violations.is_empty(), "{violations:#?}");
    }
}

/// Make a checkpoint due at the next tick, as if the interval had passed.
fn make_checkpoint_due(session: &mut RecordingSession, since_good: Duration) {
    session.last_checkpoint = Instant::now()
        .checked_sub(since_good)
        .expect("the monotonic clock is that far along");
}

const DUE: Duration = Duration::from_secs(CHECKPOINT_INTERVAL_S);

/// Headers and `segments.json` agree exactly after a graceful stop.
fn assert_finished(report: &StopReport) {
    let written = read_segments(&report.segments_path);
    for (channel, path) in [
        (Channel::Mic, &report.mic_path),
        (Channel::System, &report.sys_path),
    ] {
        let header = header_frames(path);
        assert!(header > 0, "{channel:?} declares its frames");
        assert_eq!(
            written.check_wav_header(channel, header).unwrap(),
            0,
            "{channel:?}: segments.json and the header agree after a stop"
        );
    }
}

/// The ticket's case: a kill anywhere in a stop.
#[test]
fn no_point_in_a_stop_leaves_a_header_ahead_of_segments_json() {
    let rig = Rig::new();
    let mut session = rig.start();
    make_checkpoint_due(&mut session, DUE);
    session.tick().unwrap();
    let report = session.stop().expect("stops cleanly");
    rig.assert_never_ahead();
    assert_finished(&report);
}

/// The ticket's case: a kill anywhere in a reopen, including while the new
/// sources start and align (the old code had already patched both headers
/// then, and wrote `segments.json` only at the end).
#[test]
fn no_point_in_a_reopen_leaves_a_header_ahead_of_segments_json() {
    let rig = Rig::new();
    let mut session = rig.start();
    session
        .reopen_with(
            segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
            || rig.source(Channel::Mic),
            || Some(rig.source(Channel::System)),
        )
        .expect("the reopen succeeds");
    let report = session.stop().expect("stops cleanly");
    rig.assert_never_ahead();
    assert_finished(&report);

    let written = read_segments(&report.segments_path);
    assert_eq!(written.segments.len(), 2);
    assert_eq!(
        written.segments[0].anchors.len(),
        1,
        "the close anchor, written once, before the headers"
    );
}

/// The same for a pause and its resume (TUR-146), which end and open a
/// segment the same way.
#[test]
fn no_point_in_a_pause_or_resume_leaves_a_header_ahead_of_segments_json() {
    let rig = Rig::new();
    let mut session = rig.start();
    session.pause_switch().set(true);
    session.tick().unwrap();
    session.pause_switch().set(false);
    session
        .apply_pause_request(
            || rig.source(Channel::Mic),
            || Some(rig.source(Channel::System)),
        )
        .unwrap();
    let report = session.stop().expect("stops cleanly");
    rig.assert_never_ahead();
    assert_finished(&report);
}

/// A system track that will not finish at the stop still leaves
/// `segments.json` written and the microphone finished, and its error comes
/// back after. The system frames claimed stop at the last checkpoint.
#[test]
fn a_system_track_that_will_not_stop_still_leaves_a_finished_recording() {
    let rig = Rig::new();
    let sys_fails = Arc::new(AtomicU32::new(0));
    let mic = rig.source(Channel::Mic);
    let sys = rig.source_failing(Channel::System, Arc::clone(&sys_fails));
    let mut session = RecordingSession::start(rig.dir(), mic, Some(sys)).unwrap();
    make_checkpoint_due(&mut session, DUE);
    session.tick().unwrap();
    let (mic_path, sys_path) = (session.mic_path.clone(), session.sys_path.clone());
    let segments_path = session.segments_path.clone();
    let sys_at_checkpoint = header_frames(&sys_path);

    sys_fails.store(1, Ordering::SeqCst);
    let error = session.stop().expect_err("the system error is reported");
    assert!(error.contains("stopping system audio"), "{error}");
    rig.assert_never_ahead();

    let written = read_segments(&segments_path);
    let mic_slack = written
        .check_wav_header(Channel::Mic, header_frames(&mic_path))
        .unwrap();
    assert_eq!(mic_slack, 0, "the microphone is finished exactly");
    assert_eq!(header_frames(&sys_path), sys_at_checkpoint);
    let sys_slack = written
        .check_wav_header(Channel::System, sys_at_checkpoint)
        .unwrap();
    assert_eq!(
        sys_slack, 0,
        "claimed up to the last checkpoint, no further"
    );
}

/// The ticket's third case: one failed checkpoint is tried again at the
/// next tick, and the recording goes on.
#[test]
fn a_single_failed_checkpoint_is_retried_and_the_recording_goes_on() {
    let rig = Rig::new();
    let mut session = rig.start();
    make_checkpoint_due(&mut session, DUE);

    rig.fail_fsyncs.store(1, Ordering::SeqCst);
    session
        .tick()
        .expect("one failure does not end the recording");
    assert!(
        !session.segments_path.exists(),
        "that checkpoint wrote nothing"
    );

    session.tick().expect("the retry succeeds");
    let written = read_segments(&session.segments_path);
    let mic_header = header_frames(&session.mic_path);
    assert!(mic_header > 0, "the retried checkpoint patched the header");
    written.check_wav_header(Channel::Mic, mic_header).unwrap();

    let report = session.stop().expect("and the recording stops cleanly");
    rig.assert_never_ahead();
    assert_finished(&report);
}

/// A disk that keeps failing still ends the recording, at the sixth
/// failure in a row.
#[test]
fn checkpoints_that_keep_failing_end_the_recording() {
    let rig = Rig::new();
    let mut session = rig.start();
    make_checkpoint_due(&mut session, DUE);
    rig.fail_fsyncs.store(u32::MAX, Ordering::SeqCst);

    for attempt in 1..retry::MAX_CONSECUTIVE_FAILURES {
        session
            .tick()
            .unwrap_or_else(|e| panic!("attempt {attempt} ended it early: {e}"));
    }
    let error = session.tick().expect_err("the sixth failure ends it");
    assert!(error.starts_with("mic fsync"), "{error}");
    assert!(error.contains("6 checkpoints in a row"), "{error}");
    rig.assert_never_ahead();
}

/// And so does one failure 30 s after the last good checkpoint.
#[test]
fn a_checkpoint_failing_thirty_seconds_after_the_last_good_one_ends_the_recording() {
    let rig = Rig::new();
    let mut session = rig.start();
    make_checkpoint_due(&mut session, retry::MAX_SINCE_GOOD);
    rig.fail_fsyncs.store(1, Ordering::SeqCst);
    assert!(session.tick().is_err());
}
