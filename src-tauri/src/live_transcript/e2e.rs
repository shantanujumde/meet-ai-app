//! End to end, with a real speech engine and no microphone.
//!
//! A stub [`AudioSource`] per channel plays a fixture WAV through a real
//! [`RecordingSession`] — so the frames go through the real tee, exactly as
//! the app's capture threads hand them over — into [`LiveTranscript`], a real
//! engine, and the real `MarkdownSink`. What comes out is checked the way the
//! TUR-96 gate asks: `transcript.md` is strict SPEC §3.4, the speakers are
//! right, the words are roughly right, the pane and the file agree, and quiet
//! audio produces nothing at all.
//!
//! Ignored by default because each needs something `just check` does not
//! promise: a whisper model on disk (`just model`), or the `meet-stt` sidecar
//! with Apple's en-US model installed (`just sidecar`, `just stt-probe`).
//!
//! ```text
//! cargo test -p meet-ai --lib live_transcript::e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The fixture WAVs are committed (TUR-50); `just fixtures` only remakes them.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use audio::session::{RecordingSession, Tees};
use audio::tee::Tee;
use audio::wav_writer::WavWriter;
use audio::{AudioSource, Channel, Error as AudioError};
use stt::{LiveUpdate, Speaker, SttEngine};

use super::*;

/// How much faster than real time the fixtures are played. Fast enough that
/// the suite is not a minute per engine, slow enough that neither engine is
/// pushed into the tee's overflow path (asserted below via the WAV/tee
/// frame counts).
const SPEED: f64 = 4.0;

/// One resampler output at 48 kHz, the size the real sources hand the tee.
const CHUNK: usize = 341;

// --- a microphone that plays a file ---------------------------------------

struct Playing {
    writer: WavWriter,
    frames: u64,
}

/// Plays `pcm` into a WAV and a tee at [`SPEED`]× real time, on its own
/// thread, the way `MicSource`'s worker does: WAV append under the lock, lock
/// released, then the tee.
struct FixtureSource {
    channel: Channel,
    pcm: Arc<Vec<i16>>,
    tee: Option<Tee>,
    shared: Arc<Mutex<Option<Playing>>>,
    running: Arc<AtomicBool>,
    played: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FixtureSource {
    fn new(channel: Channel, pcm: Vec<i16>) -> (Self, Arc<AtomicBool>) {
        let played = Arc::new(AtomicBool::new(false));
        (
            Self {
                channel,
                pcm: Arc::new(pcm),
                tee: None,
                shared: Arc::new(Mutex::new(None)),
                running: Arc::new(AtomicBool::new(false)),
                played: Arc::clone(&played),
                worker: None,
            },
            played,
        )
    }

    fn write(shared: &Mutex<Option<Playing>>, tee: Option<&Tee>, chunk: &[i16]) {
        {
            let mut guard = shared.lock().unwrap();
            let playing = guard.as_mut().expect("started");
            playing.writer.append(chunk).unwrap();
            playing.frames += chunk.len() as u64;
        }
        if let Some(tee) = tee {
            tee.offer(chunk);
        }
    }
}

impl AudioSource for FixtureSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), AudioError> {
        *self.shared.lock().unwrap() = Some(Playing {
            writer: WavWriter::create(&dest)?,
            frames: 0,
        });
        // The first buffer lands before `start` returns, as with real
        // hardware, so the session's first-buffer wait is satisfied at once.
        let first = CHUNK.min(self.pcm.len());
        Self::write(&self.shared, self.tee.as_ref(), &self.pcm[..first]);

        self.running.store(true, Ordering::Release);
        let (pcm, shared, tee, running, played) = (
            Arc::clone(&self.pcm),
            Arc::clone(&self.shared),
            self.tee.clone(),
            Arc::clone(&self.running),
            Arc::clone(&self.played),
        );
        let pace = Duration::from_secs_f64(CHUNK as f64 / 16_000.0 / SPEED);
        self.worker = Some(std::thread::spawn(move || {
            let began = Instant::now();
            for (i, chunk) in pcm[first..].chunks(CHUNK).enumerate() {
                if !running.load(Ordering::Acquire) {
                    return;
                }
                // Paced against the start, not per chunk, so sleep jitter
                // does not accumulate into a slower-than-asked playback.
                let due = pace * (i as u32 + 1);
                if let Some(wait) = due.checked_sub(began.elapsed()) {
                    std::thread::sleep(wait);
                }
                Self::write(&shared, tee.as_ref(), chunk);
            }
            played.store(true, Ordering::Release);
        }));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
        if let Some(playing) = self.shared.lock().unwrap().as_mut() {
            playing.writer.fsync_data()?;
            playing.writer.patch_header()?;
        }
        // Like the real sources: the tee goes away with the capture, which
        // is what tells the live side there is nothing more to come.
        self.tee = None;
        Ok(())
    }

    fn channel(&self) -> Channel {
        self.channel
    }

    fn position(&self) -> Option<(u64, u64)> {
        // Host time derived from the frame count, so both channels report
        // the same first-buffer instant and neither is head-padded.
        let frames = self.shared.lock().unwrap().as_ref()?.frames;
        Some((frames * 62_500, frames))
    }

    fn fsync_data(&mut self) -> Result<(), AudioError> {
        if let Some(playing) = self.shared.lock().unwrap().as_mut() {
            playing.writer.fsync_data()?;
        }
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), AudioError> {
        if let Some(playing) = self.shared.lock().unwrap().as_mut() {
            playing.writer.patch_header()?;
        }
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), AudioError> {
        if let Some(playing) = self.shared.lock().unwrap().as_mut() {
            playing.writer.prepend_silence(frames)?;
            playing.frames += frames;
        }
        if let Some(tee) = &self.tee {
            tee.offer_silence(frames);
        }
        Ok(())
    }

    fn tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }
}

// --- what the window would have been told ----------------------------------

#[derive(Default)]
struct Window {
    updates: Mutex<Vec<LiveUpdate>>,
    /// When each update arrived, alongside `updates`.
    arrived: Mutex<Vec<Instant>>,
    statuses: Mutex<Vec<Status>>,
}

impl Notify for Window {
    fn update(&self, update: &LiveUpdate) {
        self.updates.lock().unwrap().push(update.clone());
        self.arrived.lock().unwrap().push(Instant::now());
    }
    fn status(&self, status: &Status) {
        self.statuses.lock().unwrap().push(status.clone());
    }
}

// --- engines -----------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/audio/fixtures")
}

fn fixture(relative: &str) -> Vec<i16> {
    let path = fixtures_dir().join(relative);
    stt::read_wav_16k_mono(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run `just fixtures`", path.display()))
}

fn whisper() -> OpenEngine {
    let model = std::env::var("MEET_WHISPER_MODEL")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            let dir = stt::model::default_model_dir().ok()?;
            stt::model::MODELS
                .iter()
                .map(|spec| dir.join(spec.filename))
                .find(|path| path.is_file())
        })
        .expect("no whisper model on disk — run `just model`, or set MEET_WHISPER_MODEL");
    Box::new(move || {
        // What `registry::select` builds for the whisper choice, so this is
        // the configuration the app actually runs live.
        crate::platform::load_whisper(&model)
    })
}

fn apple() -> OpenEngine {
    let binary = stt::apple::AppleEngine::discover()
        .or_else(|| {
            let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/meet-stt");
            built.is_file().then_some(built)
        })
        .expect("no meet-stt sidecar — run `just sidecar`, or set MEET_STT_BIN");
    let probe = stt::apple::AppleEngine::probe(&binary, crate::engine::DEFAULT_LOCALE)
        .expect("meet-stt --probe");
    assert!(
        probe.is_usable_offline(),
        "Apple's engine is not usable offline here: {probe:?} — `just stt-install-locale`"
    );
    Box::new(move || {
        Ok(Box::new(stt::apple::AppleEngine::new(
            binary,
            crate::engine::DEFAULT_LOCALE,
        )) as Box<dyn SttEngine>)
    })
}

// --- one meeting ---------------------------------------------------------------

struct Meeting {
    dir: PathBuf,
    transcript: String,
    status: Status,
    snapshot: Snapshot,
    updates: Vec<LiveUpdate>,
    statuses: Vec<Status>,
    /// Frames each WAV holds, and frames the fixture had.
    wav_frames: [(u64, u64); 2],
    stop_took: Duration,
    /// The longest any guess stayed on screen without being settled or
    /// withdrawn, counting one still showing at Stop up to the Stop press.
    longest_guess: Duration,
    /// A guess was still on screen when Stop was pressed.
    guess_showing_at_stop: bool,
}

/// How long guesses stayed up, from the arrival times the window saw.
fn guess_lifetimes(updates: &[LiveUpdate], arrived: &[Instant], stop: Instant) -> (Duration, bool) {
    let mut longest = Duration::ZERO;
    let mut at_stop = false;
    for speaker in [Speaker::You, Speaker::Others] {
        let mut since: Option<Instant> = None;
        for (update, &at) in updates.iter().zip(arrived) {
            if update.speaker() != speaker {
                continue;
            }
            if at > stop && since.is_some() {
                at_stop = true;
            }
            match update {
                LiveUpdate::Volatile(_) => {
                    since.get_or_insert(at);
                }
                LiveUpdate::Final(_) | LiveUpdate::Dropped { .. } => {
                    if let Some(from) = since.take() {
                        longest = longest.max(at.min(stop).saturating_duration_since(from));
                    }
                }
            }
        }
    }
    (longest, at_stop)
}

/// Record `mic` and `system` as one meeting, transcribing live with `open`.
fn record(name: &str, mic: Vec<i16>, system: Vec<i16>, open: OpenEngine) -> Meeting {
    let dir = std::env::temp_dir().join(format!("meet-ai-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    // The recorder creates it empty up front (`create_meeting_folder`).
    std::fs::write(dir.join("transcript.md"), "").unwrap();

    let lengths = (mic.len() as u64, system.len() as u64);
    let (mic, mic_played) = FixtureSource::new(Channel::Mic, mic);
    let (sys, sys_played) = FixtureSource::new(Channel::System, system);
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();

    // The same order as `Recorder::start`: capture first, then the live side.
    let session = RecordingSession::start_with_tees(
        dir.join("audio"),
        Box::new(mic),
        Some(Box::new(sys)),
        Tees {
            mic: Some(mic_tee),
            sys: Some(sys_tee),
        },
    )
    .expect("the recording starts");
    assert!(session.status().has_system_audio);

    let live = LiveTranscript::default();
    let window = Arc::new(Window::default());
    let transcription = live.start(
        window.clone(),
        dir.join("transcript.md"),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        open,
    );

    let deadline = Instant::now() + Duration::from_secs(300);
    while !(mic_played.load(Ordering::Acquire) && sys_played.load(Ordering::Acquire)) {
        assert!(
            Instant::now() < deadline,
            "the fixture never finished playing"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Stop, in `Recorder::stop`'s order: audio closed first, then the
    // transcript is given its bounded chance to catch up.
    let stop_began = Instant::now();
    let report = session.stop().expect("the recording stops");
    let status = transcription.finish(STOP_TIMEOUT);
    let stop_took = stop_began.elapsed();

    let wav_frames = [
        (
            audio::wav_writer::read_header_frames(&report.mic_path).unwrap(),
            lengths.0,
        ),
        (
            audio::wav_writer::read_header_frames(&report.sys_path).unwrap(),
            lengths.1,
        ),
    ];

    let updates = window.updates.lock().unwrap().clone();
    let (longest_guess, guess_showing_at_stop) =
        guess_lifetimes(&updates, &window.arrived.lock().unwrap(), stop_began);
    let meeting = Meeting {
        longest_guess,
        guess_showing_at_stop,
        transcript: std::fs::read_to_string(dir.join("transcript.md")).unwrap(),
        dir,
        status,
        snapshot: live.snapshot(),
        updates,
        statuses: window.statuses.lock().unwrap().clone(),
        wav_frames,
        stop_took,
    };
    eprintln!(
        "--- {name}: {} ({} updates, stop took {:.1}s, longest guess {:.1}s{}) ---\n{}",
        meeting.status.engine.as_deref().unwrap_or("?"),
        meeting.updates.len(),
        meeting.stop_took.as_secs_f64(),
        meeting.longest_guess.as_secs_f64(),
        if meeting.guess_showing_at_stop {
            ", one still up at Stop"
        } else {
            ""
        },
        meeting.transcript
    );
    meeting
}

// --- what a finished meeting must look like ------------------------------------

/// SPEC §3.4, strictly: the regex, one utterance per line, whitespace already
/// collapsed, nothing empty, newline-terminated.
fn assert_spec_3_4(body: &str) -> Vec<(u64, Speaker, String)> {
    assert!(
        body.is_empty() || body.ends_with('\n'),
        "transcript.md must end with a newline: {body:?}"
    );
    body.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let well_formed = bytes.len() > 11
                && bytes[0] == b'['
                && bytes[3] == b':'
                && bytes[6] == b':'
                && bytes[9] == b']'
                && bytes[10] == b' '
                && [1, 2, 4, 5, 7, 8]
                    .iter()
                    .all(|&i| bytes[i].is_ascii_digit());
            assert!(well_formed, "not a §3.4 line: {line:?}");
            let (speaker, text) = if let Some(text) = line[11..].strip_prefix("You: ") {
                (Speaker::You, text)
            } else if let Some(text) = line[11..].strip_prefix("Others: ") {
                (Speaker::Others, text)
            } else {
                panic!("unknown speaker in {line:?}");
            };
            assert!(!text.trim().is_empty(), "empty text written: {line:?}");
            assert_eq!(
                stt::collapse_whitespace(text).as_deref(),
                Some(text),
                "whitespace not collapsed: {line:?}"
            );
            let clock = |at: usize| line[at..at + 2].parse::<u64>().unwrap();
            let start = clock(1) * 3600 + clock(4) * 60 + clock(7);
            (start, speaker, text.to_string())
        })
        .collect()
}

fn finals(updates: &[LiveUpdate]) -> Vec<LiveLine> {
    updates
        .iter()
        .filter_map(|update| match update {
            LiveUpdate::Final(line) => Some(line.clone()),
            _ => None,
        })
        .collect()
}

/// Everything that must hold for any meeting, speech or not.
fn assert_consistent(meeting: &Meeting) -> Vec<(u64, Speaker, String)> {
    assert_eq!(
        meeting.status.state,
        State::Stopped,
        "transcription did not finish cleanly: {:?}",
        meeting.status
    );
    assert!(
        meeting.stop_took < STOP_TIMEOUT,
        "stop took {:?}",
        meeting.stop_took
    );
    let states: Vec<State> = meeting.statuses.iter().map(|s| s.state).collect();
    assert_eq!(states, [State::Idle, State::Running, State::Stopped]);

    // The WAV holds every frame the fixture had: the tee cost capture
    // nothing, and nothing was head-padded.
    for (wav, fixture) in meeting.wav_frames {
        assert_eq!(wav, fixture, "the WAV and the fixture disagree");
    }

    let on_disk = assert_spec_3_4(&meeting.transcript);

    // Stop sorted the file by time (SPEC A19).
    assert!(
        on_disk.is_sorted_by_key(|(start, _, _)| *start),
        "transcript.md is not in time order after Stop"
    );

    // Every final the window was told about is a line on disk, and nothing
    // is on disk the window was not told about. The window heard them in
    // settle order; the file is that order, stably sorted by time.
    let told = finals(&meeting.updates);
    let mut told_utterances: Vec<_> = told.iter().map(LiveLine::to_utterance).collect();
    told_utterances.sort_by_key(|utterance| utterance.start_sec);
    let told_lines: Vec<String> = told_utterances
        .iter()
        .map(stt::format_transcript_line)
        .collect();
    let disk_lines: Vec<String> = meeting.transcript.lines().map(str::to_string).collect();
    assert_eq!(
        told_lines, disk_lines,
        "the pane's settled lines, in time order, and transcript.md differ"
    );

    // The snapshot a freshly opened window would get says the same.
    let mut snapshot_seqs: Vec<u64> = meeting.snapshot.finals.iter().map(|l| l.seq).collect();
    snapshot_seqs.sort_unstable();
    let mut told_seqs: Vec<u64> = told.iter().map(|l| l.seq).collect();
    told_seqs.sort_unstable();
    assert_eq!(snapshot_seqs, told_seqs, "snapshot and events disagree");
    assert!(
        meeting.snapshot.volatile.is_empty(),
        "stale tail after stop"
    );

    // `seq` is meeting-global and never repeats; per speaker it only rises,
    // which is what the pane's stale-update rule relies on.
    let mut all: Vec<u64> = meeting.updates.iter().map(LiveUpdate::seq).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), meeting.updates.len(), "a seq was reused");
    for speaker in [Speaker::You, Speaker::Others] {
        let seqs: Vec<u64> = meeting
            .updates
            .iter()
            .filter(|u| u.speaker() == speaker)
            .map(LiveUpdate::seq)
            .collect();
        assert!(
            seqs.windows(2).all(|w| w[0] < w[1]),
            "{speaker:?}'s updates reached the window out of seq order: {seqs:?}"
        );
    }

    // No volatile tail was left showing for either speaker.
    for speaker in [Speaker::You, Speaker::Others] {
        let mut tail = false;
        for update in meeting.updates.iter().filter(|u| u.speaker() == speaker) {
            tail = matches!(update, LiveUpdate::Volatile(_));
        }
        assert!(!tail, "{speaker:?} was left with a guess on screen");
    }
    on_disk
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn word_error_rate(reference: &[String], hypothesis: &[String]) -> f64 {
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut current = vec![0; hypothesis.len() + 1];
    for (i, r) in reference.iter().enumerate() {
        current[0] = i + 1;
        for (j, h) in hypothesis.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(r != h))
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[hypothesis.len()] as f64 / reference.len().max(1) as f64
}

#[derive(serde::Deserialize)]
struct Reference {
    utterances: Vec<ReferenceLine>,
}

#[derive(serde::Deserialize)]
struct ReferenceLine {
    start_sec: u64,
    speaker: Speaker,
    text: String,
}

/// The two-speaker fixture through `open`: the speech half of the gate.
fn speech_gate(engine: &str, open: OpenEngine) {
    let meeting = record(
        &format!("{engine}-speech"),
        fixture("two-speaker-60s/mic.wav"),
        fixture("two-speaker-60s/system.wav"),
        open,
    );
    assert_eq!(meeting.status.engine.as_deref(), Some(engine));
    let lines = assert_consistent(&meeting);
    assert!(!lines.is_empty(), "the speech fixture produced no lines");

    let reference: Reference = serde_json::from_str(
        &std::fs::read_to_string(fixtures_dir().join("two-speaker-60s/reference.json")).unwrap(),
    )
    .unwrap();

    for speaker in [Speaker::You, Speaker::Others] {
        let expected: Vec<String> = reference
            .utterances
            .iter()
            .filter(|u| u.speaker == speaker)
            .flat_map(|u| words(&u.text))
            .collect();
        // Stop sorted the file by time (SPEC A19), so this speaker's lines are
        // already in speech order, like the reference.
        let mine: Vec<&(u64, Speaker, String)> =
            lines.iter().filter(|(_, s, _)| *s == speaker).collect();
        assert!(
            mine.is_sorted_by_key(|(start, _, _)| *start),
            "{engine} {speaker:?}: transcript.md is not in time order after Stop"
        );
        let heard: Vec<String> = mine.iter().flat_map(|(_, _, text)| words(text)).collect();
        let wer = word_error_rate(&expected, &heard);
        eprintln!("{engine} live {speaker:?}: WER {:.1}%", wer * 100.0);
        // Loose on purpose: the batch gate (`accuracy.rs`) holds the engines
        // to account for accuracy. This is "the right words on the right
        // speaker", which a swapped or silent track fails at ~100%.
        assert!(wer < 0.35, "{engine} {speaker:?} WER {:.0}%", wer * 100.0);

        // Timestamps are on the WAV timeline: each line starts near one of
        // that speaker's reference utterances.
        for (start, _, text) in &mine {
            let near = reference.utterances.iter().any(|u| {
                u.speaker == speaker && *start + 2 >= u.start_sec && *start <= u.start_sec + 5
            });
            assert!(
                near,
                "{engine}: {speaker:?} line at {start}s is nowhere near what they said: {text:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&meeting.dir);
}

/// Quiet on both tracks: nothing written, nothing guessed (TUR-67, live).
fn silence_gate(engine: &str, open: impl Fn() -> OpenEngine) {
    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let meeting = record(
            &format!("{engine}-{}", name.trim_end_matches(".wav")),
            fixture(name),
            fixture(name),
            open(),
        );
        assert_consistent(&meeting);
        assert_eq!(
            meeting.transcript, "",
            "{engine} invented text over {name}: {:?}",
            meeting.transcript
        );
        assert!(
            finals(&meeting.updates).is_empty(),
            "{engine} settled a line over {name}"
        );
        // Apple's model may guess at room tone for a moment (measured: one
        // "I"). It must be brief — gone within `STALE_GUESS` of audio — and
        // never still up when the meeting ends. Whisper guesses nothing.
        let limit = STALE_GUESS.div_f64(SPEED) + Duration::from_secs(1);
        assert!(
            !meeting.guess_showing_at_stop,
            "{engine} left a guess on screen over {name} until Stop: {:?}",
            meeting.updates
        );
        assert!(
            meeting.longest_guess <= limit,
            "{engine} kept a guess up for {:?} over {name} (limit {limit:?}): {:?}",
            meeting.longest_guess,
            meeting.updates
        );
        if engine == "whisper" {
            assert!(
                meeting.updates.is_empty(),
                "whisper put text on screen over {name}: {:?}",
                meeting.updates
            );
        }
        let _ = std::fs::remove_dir_all(&meeting.dir);
    }
}

#[test]
#[ignore = "needs a whisper model on disk: `just model`, `just fixtures`"]
fn whisper_live_meeting_end_to_end() {
    speech_gate("whisper", whisper());
}

#[test]
#[ignore = "needs a whisper model on disk: `just model`, `just fixtures`"]
fn whisper_live_quiet_meeting_writes_nothing() {
    silence_gate("whisper", whisper);
}

#[test]
#[ignore = "needs the meet-stt sidecar and Apple's en-US model: `just sidecar`, `just fixtures`"]
fn apple_live_meeting_end_to_end() {
    speech_gate("apple-speech", apple());
}

#[test]
#[ignore = "needs the meet-stt sidecar and Apple's en-US model: `just sidecar`, `just fixtures`"]
fn apple_live_quiet_meeting_writes_nothing() {
    silence_gate("apple-speech", apple);
}
