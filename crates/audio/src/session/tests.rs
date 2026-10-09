use std::sync::{Arc, Mutex};

use super::*;
use crate::segments::SegmentsDrift as _;
use crate::wav_writer::WavWriter;

/// A hardware-free `AudioSource` for exercising [`align_and_pad`]'s
/// alignment maths, which is the one piece of the device-change handling
/// that does not itself need Core Audio or `cpal` — everything else in
/// [`reopen_segment`] is OS integration, verified on real hardware
/// instead (see `crates/audio/tests/*_closed_loop.rs`).
///
/// `padded_frames` is an `Arc` specifically so a test can keep its own
/// handle to it after the `FakeSource` has been moved into a
/// `Box<dyn AudioSource>` — taking a raw reference to a field and moving
/// the struct afterward would leave that reference dangling.
struct FakeSource {
    channel: Channel,
    position: Option<(u64, u64)>,
    padded_frames: Arc<Mutex<Option<u64>>>,
    /// Set by `stop()`, so a test can check a given-up source was stopped.
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl FakeSource {
    fn new(channel: Channel, position: Option<(u64, u64)>) -> Self {
        Self {
            channel,
            position,
            padded_frames: Arc::new(Mutex::new(None)),
            stopped: Arc::default(),
        }
    }
}

impl AudioSource for FakeSource {
    fn start(&mut self, _dest: PathBuf) -> Result<(), AudioError> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), AudioError> {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn channel(&self) -> Channel {
        self.channel
    }
    fn position(&self) -> Option<(u64, u64)> {
        self.position
    }
    fn device_rate(&self) -> Option<u32> {
        Some(44_100)
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

/// A hardware-free `AudioSource` that writes a real, playable WAV — SPEC
/// §8.2's Windows "stub-audio" shape, reused here so a session can be
/// exercised start-to-stop without Core Audio or a microphone TCC grant.
/// Unlike [`FakeSource`] above (which only fakes `position()` to test
/// alignment maths in isolation), this one really appends samples through
/// a real [`WavWriter`], so [`RecordingSession::stop`]'s output is real
/// bytes on disk, not an assumption.
struct StubSource {
    channel: Channel,
    writer: Option<WavWriter>,
    frames: u64,
    started: Option<Instant>,
    tee: Option<Tee>,
}

impl StubSource {
    fn new(channel: Channel) -> Self {
        Self {
            channel,
            writer: None,
            frames: 0,
            started: None,
            tee: None,
        }
    }

    /// One buffer of silence, appended synchronously — a real capture
    /// device warms up over milliseconds to seconds, but nothing this
    /// test cares about needs that delay reproduced.
    const BUFFER_FRAMES: u64 = 160; // 10ms at 16kHz

    fn append_buffer(&mut self) -> Result<(), AudioError> {
        let writer = self.writer.as_mut().expect("started before appending");
        let buffer = vec![0i16; Self::BUFFER_FRAMES as usize];
        writer.append(&buffer)?;
        self.frames += Self::BUFFER_FRAMES;
        if let Some(tee) = &self.tee {
            tee.offer(&buffer);
        }
        Ok(())
    }
}

impl AudioSource for StubSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), AudioError> {
        let writer = if dest.exists() {
            WavWriter::open_append(&dest)?
        } else {
            WavWriter::create(&dest)?
        };
        self.writer = Some(writer);
        self.frames = 0;
        self.append_buffer()?;
        self.started = Some(Instant::now());
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.fsync_data()?;
            writer.patch_header()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        self.channel
    }

    fn position(&self) -> Option<(u64, u64)> {
        let started = self.started?;
        Some((started.elapsed().as_nanos() as u64, self.frames))
    }

    fn fsync_data(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.fsync_data()?;
        }
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.patch_header()?;
        }
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.prepend_silence(frames)?;
            self.frames += frames;
        }
        if let Some(tee) = &self.tee {
            tee.offer_silence(frames);
        }
        Ok(())
    }

    fn tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }

    fn device_rate(&self) -> Option<u32> {
        Some(48_000)
    }
}

/// The gate this ticket names: start a session against a stub source for
/// each channel, stop it, and get back complete WAVs and a valid
/// `segments.json` — the whole start/tick/stop lifecycle exercised
/// without any real hardware.
#[test]
fn a_session_against_stub_sources_produces_complete_wavs_and_valid_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let sys: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::System));
    let mut session =
        RecordingSession::start(dir.clone(), mic, Some(sys)).expect("session starts cleanly");

    assert!(session.status().has_system_audio);

    // Exercise `tick()` too, even though nothing is due yet (no device
    // watch off macOS, no checkpoint inside 5s) — it must be a harmless
    // no-op rather than something the caller has to avoid calling early.
    session
        .tick()
        .expect("an early tick is a no-op, not an error");

    let report = session.stop().expect("session stops cleanly");
    assert!(report.has_system_audio);

    let mic_frames =
        crate::wav_writer::read_header_frames(&report.mic_path).expect("mic.wav is playable");
    let sys_frames =
        crate::wav_writer::read_header_frames(&report.sys_path).expect("system.wav is playable");
    assert!(mic_frames > 0, "mic.wav must declare real frames");
    assert!(sys_frames > 0, "system.wav must declare real frames");

    let json = std::fs::read_to_string(&report.segments_path).expect("segments.json was written");
    let segments = crate::segments::Segments::from_json(&json)
        .expect("segments.json must parse as the §3.4 shape");
    segments
        .check_wav_header(Channel::Mic, mic_frames)
        .expect("segments.json must account for at least what mic.wav declares");
    segments
        .check_wav_header(Channel::System, sys_frames)
        .expect("segments.json must account for at least what system.wav declares");
}

/// Everything that went into a tee, drained after the session stopped.
fn drain(feed: &crate::tee::TeeFeed) -> u64 {
    let mut frames = 0;
    while let Ok(chunk) = feed.recv_timeout(Duration::ZERO) {
        frames += chunk.len() as u64;
    }
    frames
}

/// TUR-96's hook: each channel's tee gets exactly the frames its WAV got,
/// head-pad included, so a live transcript's timestamps land on the same
/// timeline as the file a batch re-run would read.
#[test]
fn a_teed_session_hands_each_channel_exactly_what_its_wav_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let (mic_tee, mic_feed) = crate::tee::tee();
    let (sys_tee, sys_feed) = crate::tee::tee();

    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let sys: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::System));
    let session = RecordingSession::start_with_tees(
        dir.clone(),
        mic,
        Some(sys),
        Tees {
            mic: Some(mic_tee),
            sys: Some(sys_tee),
        },
    )
    .expect("session starts cleanly");
    let report = session.stop().expect("session stops cleanly");

    let mic_wav = crate::wav_writer::read_header_frames(&report.mic_path).unwrap();
    let sys_wav = crate::wav_writer::read_header_frames(&report.sys_path).unwrap();
    assert!(mic_wav > 0 && sys_wav > 0);
    assert_eq!(drain(&mic_feed), mic_wav, "mic tee and mic.wav disagree");
    assert_eq!(
        drain(&sys_feed),
        sys_wav,
        "system tee and system.wav disagree"
    );

    // The session is gone, so every tee clone is too: the feed reports
    // that as the end of the recording rather than waiting forever.
    assert!(matches!(
        mic_feed.recv_timeout(Duration::ZERO),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
}

/// The independence property (TUR-31, TUR-96): the speech side vanishing
/// before the recording even starts changes nothing about the recording.
#[test]
fn a_tee_nobody_reads_changes_nothing_about_the_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let (mic_tee, mic_feed) = crate::tee::tee_with_capacity(1);
    drop(mic_feed);

    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let session = RecordingSession::start_with_tees(
        dir.clone(),
        mic,
        None,
        Tees {
            mic: Some(mic_tee),
            sys: None,
        },
    )
    .expect("a dead tee must not stop a recording from starting");
    let report = session.stop().expect("or from stopping");
    assert!(crate::wav_writer::read_header_frames(&report.mic_path).unwrap() > 0);
}

/// The crash-safety half of TUR-97: a recording that is ticked but never
/// stopped — `kill -9`, a crash, a power cut — must still leave WAVs whose
/// headers declare real frames and a `segments.json` that accounts for
/// them, because the checkpoint `tick()` runs is the only thing that
/// writes either before `stop()`. The app shipped 0.3.0 without ever
/// calling `tick()`, and a killed recording left 0-byte headers over ~16 s
/// of PCM and no `segments.json`; this pins the session half of the fix.
///
/// Backdates `last_checkpoint` instead of sleeping out
/// `CHECKPOINT_INTERVAL_S`, and ends with `mem::forget` rather than
/// `stop()` or a drop, so nothing after the tick can patch a header or
/// write `segments.json` — the same bytes a killed process leaves behind.
#[test]
fn a_due_tick_checkpoints_so_a_session_that_never_stops_is_still_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let sys: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::System));
    let mut session =
        RecordingSession::start(dir.clone(), mic, Some(sys)).expect("session starts cleanly");
    let segments_path = session.segments_path.clone();
    let (mic_path, sys_path) = (session.mic_path.clone(), session.sys_path.clone());

    assert!(
        !segments_path.exists(),
        "segments.json is first written by a checkpoint, never by start"
    );

    session
        .tick()
        .expect("an early tick is a no-op, not an error");
    assert!(
        !segments_path.exists(),
        "a tick inside the checkpoint interval must not checkpoint"
    );

    session.last_checkpoint = Instant::now()
        .checked_sub(Duration::from_secs(CHECKPOINT_INTERVAL_S))
        .expect("the monotonic clock is past one checkpoint interval");
    session.tick().expect("a due checkpoint succeeds");

    // Simulate the process dying here: no stop, no Drop.
    std::mem::forget(session);

    let mic_frames = crate::wav_writer::read_header_frames(&mic_path).expect("mic.wav is playable");
    let sys_frames =
        crate::wav_writer::read_header_frames(&sys_path).expect("system.wav is playable");
    assert!(mic_frames > 0, "the checkpoint must patch mic.wav's header");
    assert!(
        sys_frames > 0,
        "the checkpoint must patch system.wav's header"
    );

    let json =
        std::fs::read_to_string(&segments_path).expect("the checkpoint must write segments.json");
    let segments = crate::segments::Segments::from_json(&json)
        .expect("segments.json must parse as the §3.4 shape");
    segments
        .check_wav_header(Channel::Mic, mic_frames)
        .expect("segments.json must account for at least what mic.wav declares");
    segments
        .check_wav_header(Channel::System, sys_frames)
        .expect("segments.json must account for at least what system.wav declares");
}

/// A caller with no system-audio implementation on this platform yet
/// (SPEC §8.2's Windows stub) must still get a complete recording —
/// contract §9's "absent track" path, not an error.
#[test]
fn a_session_with_no_system_source_still_produces_a_complete_mic_only_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let session = RecordingSession::start(dir.clone(), mic, None).expect("session starts cleanly");
    assert!(!session.status().has_system_audio);

    let report = session.stop().expect("session stops cleanly");
    assert!(!report.has_system_audio);

    let mic_frames =
        crate::wav_writer::read_header_frames(&report.mic_path).expect("mic.wav is playable");
    assert!(mic_frames > 0);
    assert!(
        !report.sys_path.exists(),
        "no system-audio source means system.wav is never created"
    );
}

fn stopped(flag: &Arc<std::sync::atomic::AtomicBool>) -> bool {
    flag.load(std::sync::atomic::Ordering::SeqCst)
}

fn read_segments(path: &Path) -> crate::segments::Segments {
    let json = std::fs::read_to_string(path).expect("segments.json is written");
    crate::segments::Segments::from_json(&json).expect("segments.json parses")
}

/// TUR-87 C3: a tap that starts but never delivers a first frame within the
/// budget is stopped and dropped, and alignment goes on with the mic alone.
#[test]
fn align_and_pad_drops_a_system_track_whose_first_frame_is_late() {
    let mut mic = FakeSource::new(Channel::Mic, Some((1_000_000_000, 0)));
    let late = FakeSource::new(Channel::System, None);
    let late_stopped = Arc::clone(&late.stopped);
    let mut sys: Option<Box<dyn AudioSource>> = Some(Box::new(late));

    let start = align_and_pad(&mut mic, &mut sys).expect("a late system track is not an error");

    assert_eq!(start, 1_000_000_000);
    assert!(sys.is_none(), "the late system track is dropped");
    assert!(stopped(&late_stopped), "and stopped, so its worker ends");
}

/// TUR-87 C3, at the session level: Record still records the mic.
#[test]
fn a_late_system_track_at_start_records_microphone_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let late = FakeSource::new(Channel::System, None);
    let late_stopped = Arc::clone(&late.stopped);

    let session = RecordingSession::start(tmp.path().to_path_buf(), mic, Some(Box::new(late)))
        .expect("the recording starts microphone-only");
    assert!(!session.status().has_system_audio);
    assert!(stopped(&late_stopped));

    let report = session.stop().expect("stops cleanly");
    assert!(!report.has_system_audio);
    let segments = read_segments(&report.segments_path);
    assert_eq!(
        segments.segments[0].sys_rate, 0,
        "absent track (contract §9)"
    );
    assert!(crate::wav_writer::read_header_frames(&report.mic_path).unwrap() > 0);
}

/// A microphone with no first frame still fails the start, and nothing
/// started is left running.
#[test]
fn a_silent_microphone_fails_the_start_and_stops_both_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let mic = FakeSource::new(Channel::Mic, None);
    let mic_stopped = Arc::clone(&mic.stopped);
    let sys = FakeSource::new(Channel::System, Some((1, 0)));
    let sys_stopped = Arc::clone(&sys.stopped);

    let result =
        RecordingSession::start(tmp.path().to_path_buf(), Box::new(mic), Some(Box::new(sys)));

    assert!(result.is_err());
    assert!(stopped(&mic_stopped) && stopped(&sys_stopped));
}

/// TUR-87 L4: each segment records the device rates it resampled from.
#[test]
fn segments_json_records_both_device_rates() {
    let tmp = tempfile::tempdir().unwrap();
    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let sys: Box<dyn AudioSource> = Box::new(FakeSource::new(Channel::System, Some((1, 0))));
    let session = RecordingSession::start(tmp.path().to_path_buf(), mic, Some(sys)).unwrap();
    let report = session.stop().unwrap();

    let segment = &read_segments(&report.segments_path).segments[0];
    assert_eq!(segment.mic_device_rate, Some(48_000));
    assert_eq!(segment.sys_device_rate, Some(44_100));
}

/// A started session's parts, for driving [`reopen_segment`] by hand.
struct Reopen {
    _tmp: tempfile::TempDir,
    mic: Box<dyn AudioSource>,
    sys: Option<Box<dyn AudioSource>>,
    writer: SegmentsWriter,
    dir: PathBuf,
    want_sys: bool,
}

impl Reopen {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let mut mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
        mic.start(dir.join("mic.wav")).unwrap();
        let mut sys: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::System));
        sys.start(dir.join("system.wav")).unwrap();
        let open = segment_open(1, &*mic, Some(&*sys), segments::reason::START);
        Self {
            _tmp: tmp,
            mic,
            sys: Some(sys),
            writer: SegmentsWriter::new(open),
            dir,
            want_sys: true,
        }
    }

    /// A session whose system track is already down (or never asked for).
    fn without_sys(want_sys: bool) -> Self {
        let mut r = Self::new();
        if let Some(mut s) = r.sys.take() {
            s.stop().unwrap();
        }
        r.want_sys = want_sys;
        r
    }

    fn run(
        &mut self,
        new_mic: Box<dyn AudioSource>,
        new_sys: Option<Box<dyn AudioSource>>,
    ) -> Result<(), String> {
        let (segments, mic, sys) = (
            self.dir.join("segments.json"),
            self.dir.join("mic.wav"),
            self.dir.join("system.wav"),
        );
        let paths = Paths {
            segments: &segments,
            mic: &mic,
            sys: &sys,
        };
        reopen_segment(
            &mut self.mic,
            &mut self.sys,
            &mut self.writer,
            &paths,
            segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
            &Tees::default(),
            self.want_sys,
            || new_mic,
            || new_sys,
        )
    }
}

/// TUR-87 C3: an AirPods swap whose new tap is slow keeps recording the mic.
#[test]
fn a_reopen_whose_new_tap_is_late_continues_microphone_only() {
    let mut r = Reopen::new();
    let late = FakeSource::new(Channel::System, None);
    let late_stopped = Arc::clone(&late.stopped);

    r.run(
        Box::new(StubSource::new(Channel::Mic)),
        Some(Box::new(late)),
    )
    .expect("a late tap does not end the recording");

    assert!(r.sys.is_none());
    assert!(stopped(&late_stopped));
    let segments = read_segments(&r.dir.join("segments.json"));
    assert_eq!(segments.segments.len(), 2);
    assert_eq!(segments.segments[1].sys_rate, 0);
}

/// A reopen whose new microphone never delivers fails, but stops every
/// source it started first (the old code dropped them running).
#[test]
fn a_failed_reopen_stops_the_sources_it_started() {
    let mut r = Reopen::new();
    let mic = FakeSource::new(Channel::Mic, None);
    let mic_stopped = Arc::clone(&mic.stopped);
    let sys = FakeSource::new(Channel::System, Some((5, 0)));
    let sys_stopped = Arc::clone(&sys.stopped);

    assert!(r.run(Box::new(mic), Some(Box::new(sys))).is_err());
    assert!(stopped(&mic_stopped), "new mic stopped");
    assert!(stopped(&sys_stopped), "new tap stopped");
}

fn sys_rates(r: &Reopen) -> Vec<u32> {
    read_segments(&r.dir.join("segments.json"))
        .segments
        .iter()
        .map(|s| s.sys_rate)
        .collect()
}

/// TUR-121: a tap that was down at start comes back at the next reopen.
#[test]
fn a_system_track_down_at_start_returns_at_the_next_reopen() {
    let mut r = Reopen::without_sys(true);
    r.run(
        Box::new(StubSource::new(Channel::Mic)),
        Some(Box::new(StubSource::new(Channel::System))),
    )
    .unwrap();
    assert!(r.sys.is_some());
    assert!(sys_rates(&r)[1] > 0);
}

/// TUR-121: a tap lost at one reopen is rebuilt at the following one.
#[test]
fn a_system_track_lost_at_a_reopen_returns_at_the_following_one() {
    let mut r = Reopen::new();
    r.run(
        Box::new(StubSource::new(Channel::Mic)),
        Some(Box::new(FakeSource::new(Channel::System, None))),
    )
    .unwrap();
    assert!(r.sys.is_none());
    r.run(
        Box::new(StubSource::new(Channel::Mic)),
        Some(Box::new(StubSource::new(Channel::System))),
    )
    .unwrap();
    assert!(r.sys.is_some());
    let rates = sys_rates(&r);
    assert_eq!(rates.len(), 3);
    assert_eq!(rates[1], 0);
    assert!(rates[2] > 0);
}

/// TUR-121: a session that never asked for system audio never builds it.
#[test]
fn a_session_without_system_audio_never_builds_one() {
    let mut r = Reopen::without_sys(false);
    let mut built = false;
    let (segments, mic, sys) = (
        r.dir.join("segments.json"),
        r.dir.join("mic.wav"),
        r.dir.join("system.wav"),
    );
    let paths = Paths {
        segments: &segments,
        mic: &mic,
        sys: &sys,
    };
    reopen_segment(
        &mut r.mic,
        &mut r.sys,
        &mut r.writer,
        &paths,
        segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
        &Tees::default(),
        r.want_sys,
        || Box::new(StubSource::new(Channel::Mic)),
        || {
            built = true;
            Some(Box::new(StubSource::new(Channel::System)))
        },
    )
    .unwrap();
    assert!(!built);
    assert!(r.sys.is_none());
}

fn stub(channel: Channel) -> Box<dyn AudioSource> {
    Box::new(StubSource::new(channel))
}

/// TUR-136: the system-audio check found the tap denied mid-recording. The
/// segment closes with `system_audio_denied`, the next one has no system
/// track, and the microphone records on into a complete, readable file.
#[test]
fn dropping_the_system_track_closes_the_segment_and_records_the_microphone_on() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = RecordingSession::start(
        tmp.path().to_path_buf(),
        stub(Channel::Mic),
        Some(stub(Channel::System)),
    )
    .expect("session starts cleanly");
    assert!(session.status().has_system_audio);

    // Asked from another thread, carried out by the next tick.
    let asker = session.system_drop();
    asker.request(segments::reason::SYSTEM_AUDIO_DENIED);
    session
        .apply_drop_request(|| stub(Channel::Mic))
        .expect("the drop never fails while the mic restarts");

    assert!(!session.status().has_system_audio, "mic-only from here on");
    assert!(
        !session.wants_system_audio(),
        "a denied tap is not wanted back"
    );

    // A later device change must not rebuild the tap the check found denied.
    let mut rebuilt = false;
    session
        .reopen_with(
            segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
            || stub(Channel::Mic),
            || {
                rebuilt = true;
                Some(stub(Channel::System))
            },
        )
        .unwrap();
    assert!(!rebuilt && !session.status().has_system_audio);

    let report = session.stop().expect("the recording still stops cleanly");
    assert!(!report.has_system_audio);
    let written = read_segments(&report.segments_path);
    let reasons: Vec<&str> = written.segments.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            segments::reason::START,
            segments::reason::SYSTEM_AUDIO_DENIED,
            segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
        ]
    );
    let sys_rates: Vec<u32> = written.segments.iter().map(|s| s.sys_rate).collect();
    assert_eq!(
        sys_rates,
        [crate::segments::SAMPLE_RATE_HZ, 0, 0],
        "absent track, contract §9"
    );

    let mic_frames = crate::wav_writer::read_header_frames(&report.mic_path).unwrap();
    let sys_frames = crate::wav_writer::read_header_frames(&report.sys_path).unwrap();
    assert!(written.segments.iter().all(|s| s.mic_frames > 0));
    written
        .check_wav_header(Channel::Mic, mic_frames)
        .expect("segments.json accounts for every mic frame");
    written
        .check_wav_header(Channel::System, sys_frames)
        .expect("and for the system frames before the drop");
}

#[test]
fn a_session_with_no_drop_request_ticks_on_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = RecordingSession::start(
        tmp.path().to_path_buf(),
        stub(Channel::Mic),
        Some(stub(Channel::System)),
    )
    .unwrap();
    let mut built = false;
    session
        .apply_drop_request(|| {
            built = true;
            stub(Channel::Mic)
        })
        .unwrap();
    assert!(!built, "nothing restarts without a request");
    assert!(session.status().has_system_audio && session.wants_system_audio());
    let report = session.stop().unwrap();
    assert_eq!(read_segments(&report.segments_path).segments.len(), 1);
}

#[test]
fn dropping_a_system_track_that_is_already_gone_only_stops_wanting_it() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session =
        RecordingSession::start(tmp.path().to_path_buf(), stub(Channel::Mic), None).unwrap();
    session
        .drop_system_with(segments::reason::SYSTEM_AUDIO_DENIED, || {
            panic!("no segment to close, so the mic is not restarted")
        })
        .unwrap();
    assert!(!session.wants_system_audio());
    let report = session.stop().unwrap();
    assert_eq!(read_segments(&report.segments_path).segments.len(), 1);
}

/// The header frames of the WAV at `path`.
fn header_frames(path: &Path) -> u64 {
    crate::wav_writer::read_header_frames(path).expect("the WAV is playable")
}

/// TUR-146: a pause stops both channels, so nothing reaches the WAVs or the
/// live-transcript tee until the resume; the resume carries on in the same
/// files, one meeting with a new `resumed_after_pause` segment.
#[test]
fn a_pause_writes_nothing_and_a_resume_carries_on_in_the_same_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (mic_tee, mic_feed) = crate::tee::tee();
    let mut session = RecordingSession::start_with_tees(
        tmp.path().to_path_buf(),
        stub(Channel::Mic),
        Some(stub(Channel::System)),
        Tees {
            mic: Some(mic_tee),
            sys: None,
        },
    )
    .expect("session starts cleanly");
    let switch = session.pause_switch();

    switch.set(true);
    session.tick().expect("the tick carries out the pause");
    assert!(session.is_paused());
    let at_pause = read_segments(&session.segments_path);
    assert_eq!(at_pause.segments.len(), 1, "a pause opens no segment");
    let mic_at_pause = header_frames(&session.mic_path);
    assert!(mic_at_pause > 0);
    assert_eq!(
        at_pause.segments[0].mic_frames, mic_at_pause,
        "segments.json is written at the pause, so a crash mid-pause loses nothing"
    );

    // Paused, even a due checkpoint does nothing: there is nothing new.
    session.last_checkpoint = Instant::now()
        .checked_sub(Duration::from_secs(CHECKPOINT_INTERVAL_S))
        .expect("the monotonic clock is past one checkpoint interval");
    session.tick().expect("a paused tick is a no-op");
    assert_eq!(read_segments(&session.segments_path), at_pause);
    assert_eq!(header_frames(&session.mic_path), mic_at_pause);
    assert_eq!(
        drain(&mic_feed),
        mic_at_pause,
        "the tee got only what the WAV holds, nothing for the pause"
    );

    switch.set(false);
    let paused = session
        .apply_pause_request(|| stub(Channel::Mic), || Some(stub(Channel::System)))
        .expect("the resume starts both channels again");
    assert!(!paused && !session.is_paused());
    assert!(session.status().has_system_audio);

    let report = session.stop().expect("stops cleanly");
    let written = read_segments(&report.segments_path);
    let reasons: Vec<&str> = written.segments.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            segments::reason::START,
            segments::reason::RESUMED_AFTER_PAUSE
        ]
    );
    assert!(written.segments.iter().all(|s| s.sys_rate > 0));
    let mic_frames = header_frames(&report.mic_path);
    assert!(mic_frames > mic_at_pause, "one mic.wav, appended to");
    written
        .check_wav_header(Channel::Mic, mic_frames)
        .expect("segments.json accounts for every mic frame");
    written
        .check_wav_header(Channel::System, header_frames(&report.sys_path))
        .expect("and every system frame");
    assert_eq!(drain(&mic_feed), mic_frames - mic_at_pause);
}

/// Stop while paused finishes the files exactly as the pause left them.
#[test]
fn stopping_while_paused_finishes_the_files_as_the_pause_left_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = RecordingSession::start(
        tmp.path().to_path_buf(),
        stub(Channel::Mic),
        Some(stub(Channel::System)),
    )
    .unwrap();
    session.pause_switch().set(true);
    session.tick().unwrap();
    let mic_at_pause = header_frames(&session.mic_path);

    let report = session.stop().expect("a paused session stops cleanly");
    let written = read_segments(&report.segments_path);
    assert_eq!(written.segments.len(), 1);
    assert_eq!(header_frames(&report.mic_path), mic_at_pause);
    written
        .check_wav_header(Channel::Mic, mic_at_pause)
        .expect("segments.json matches mic.wav");
}

/// TUR-136 meets TUR-146: a system-audio denial found during a pause is
/// honoured at the resume, which builds no tap.
#[test]
fn a_system_drop_asked_for_during_a_pause_is_honoured_at_the_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = RecordingSession::start(
        tmp.path().to_path_buf(),
        stub(Channel::Mic),
        Some(stub(Channel::System)),
    )
    .unwrap();
    let switch = session.pause_switch();
    switch.set(true);
    session.tick().unwrap();
    session
        .system_drop()
        .request(segments::reason::SYSTEM_AUDIO_DENIED);

    switch.set(false);
    let mut built = false;
    session
        .apply_pause_request(
            || stub(Channel::Mic),
            || {
                built = true;
                Some(stub(Channel::System))
            },
        )
        .unwrap();
    assert!(!built, "no tap for a denied system track");
    assert!(!session.status().has_system_audio && !session.wants_system_audio());
    let report = session.stop().unwrap();
    let written = read_segments(&report.segments_path);
    let sys_rates: Vec<u32> = written.segments.iter().map(|s| s.sys_rate).collect();
    assert_eq!(sys_rates, [crate::segments::SAMPLE_RATE_HZ, 0]);
}

/// A microphone that will not restart fails the resume, stops what it
/// started, and leaves the session paused and still stoppable.
#[test]
fn a_microphone_that_will_not_restart_fails_the_resume_and_stays_paused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session =
        RecordingSession::start(tmp.path().to_path_buf(), stub(Channel::Mic), None).unwrap();
    let switch = session.pause_switch();
    switch.set(true);
    session.tick().unwrap();

    switch.set(false);
    let silent = FakeSource::new(Channel::Mic, None);
    let silent_stopped = Arc::clone(&silent.stopped);
    assert!(
        session
            .apply_pause_request(move || Box::new(silent), || None)
            .is_err()
    );
    assert!(stopped(&silent_stopped), "the new mic was stopped");
    assert!(session.is_paused());

    let report = session.stop().expect("the files still finish");
    assert_eq!(read_segments(&report.segments_path).segments.len(), 1);
}
