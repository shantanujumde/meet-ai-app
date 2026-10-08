//! `transcription.live: false`, against a fake batch engine.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use meeting_format::layout;
use stt::{Channel, Speaker, SttEngine, TranscriptSink, Utterance};

use super::super::fakes::{CollectingNotify, Gate, read, temp_transcript, wait_for};
use super::super::{LiveTranscript, OpenEngine, Plan, State, Transcription};

/// What the fake does in `transcribe`.
#[derive(Clone)]
enum Batch {
    /// One line per track: `Others` at 1 s, `You` at 2 s.
    Lines,
    /// As `Lines`, once the gate opens: a long meeting.
    LinesAfter(Gate),
    /// Fails on the first track.
    Fails,
}

struct BatchEngine(Batch);

impl SttEngine for BatchEngine {
    fn name(&self) -> &'static str {
        "batch-fake"
    }

    fn transcribe(
        &mut self,
        _wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), stt::Error> {
        match &self.0 {
            Batch::Lines => {}
            Batch::LinesAfter(gate) => gate.wait(),
            Batch::Fails => return Err(stt::Error::Engine("the fake engine fell over".into())),
        }
        let start_sec = match speaker {
            Speaker::Others => 1,
            Speaker::You => 2,
        };
        sink.write(&Utterance {
            start_sec,
            speaker,
            text: format!("{} said this.", speaker.label()),
        })?;
        sink.flush()
    }
}

/// A batch engine, counting how often it is opened.
fn batch(mode: Batch, opened: &Arc<AtomicUsize>) -> OpenEngine {
    let opened = Arc::clone(opened);
    Box::new(move || {
        opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(BatchEngine(mode)) as Box<dyn SttEngine>)
    })
}

const BOTH_LINES: &str = "[00:00:01] Others: Others said this.\n[00:00:02] You: You said this.\n";

/// A meeting folder with both WAVs (empty: the fake never reads them) and
/// the retention marker a recording leaves, recorded with `live: false`.
/// Audio is offered to both tees, as capture would.
fn record(
    name: &str,
    open: OpenEngine,
) -> (
    super::super::fakes::TempTranscript,
    LiveTranscript,
    Arc<CollectingNotify>,
    Transcription,
) {
    let transcript = temp_transcript(name);
    let dir = transcript.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(layout::audio_dir(&dir)).unwrap();
    for channel in [Channel::Mic, Channel::System] {
        std::fs::write(layout::wav_path(&dir, channel), b"").unwrap();
    }
    store::retention::mark_incomplete(&dir).unwrap();

    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();
    let transcription = live.start(
        notify.clone(),
        transcript.path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        Plan { open, live: false },
    );
    // Capture goes on as normal; nobody is reading the copies.
    for _ in 0..3 {
        mic_tee.offer(&[1; 160]);
        sys_tee.offer(&[1; 160]);
    }
    (transcript, live, notify, transcription)
}

fn incomplete(transcript: &Path) -> bool {
    store::retention::incomplete_marker(transcript.parent().unwrap()).exists()
}

#[test]
fn live_off_records_with_no_engine_and_writes_transcript_md_only_after_stop() {
    let opened = Arc::new(AtomicUsize::new(0));
    let (transcript, live, notify, transcription) = record("off", batch(Batch::Lines, &opened));

    // During the meeting: no engine, no file, nothing on the board.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        opened.load(Ordering::SeqCst),
        0,
        "no engine while recording"
    );
    assert!(!transcript.exists(), "no transcript.md while recording");
    let snapshot = live.snapshot();
    assert_eq!(snapshot.status.state, State::Idle);
    assert!(snapshot.finals.is_empty() && snapshot.volatile.is_empty());

    let (status, mut transcript_final) = transcription.finish_final(Duration::from_secs(5));

    assert_eq!(opened.load(Ordering::SeqCst), 1, "one engine, at Stop");
    assert_eq!(status.state, State::Stopped);
    assert_eq!(status.engine.as_deref(), Some("batch-fake"));
    assert_eq!(read(&transcript), BOTH_LINES, "both tracks, in time order");
    assert!(
        transcript_final.wait(Duration::ZERO),
        "final when Stop returns"
    );
    assert_eq!(
        notify.states(),
        [State::Idle, State::Running, State::Stopped]
    );
    // The caller marks the audio complete from the status (`finish_then_run`).
    assert!(incomplete(&transcript));
}

#[test]
fn a_batch_longer_than_stop_finishes_in_the_background_and_the_notes_wait_for_it() {
    let opened = Arc::new(AtomicUsize::new(0));
    let gate = Gate::default();
    let (transcript, _live, notify, transcription) =
        record("long", batch(Batch::LinesAfter(gate.clone()), &opened));

    let (status, mut transcript_final) = transcription.finish_final(Duration::from_millis(50));

    assert_ne!(status.state, State::Stopped, "not done yet: {status:?}");
    assert_ne!(status.state, State::Failed, "a long batch is not a failure");
    assert!(!transcript.exists(), "nothing written yet");

    // The notes run's wait outlasts its own timeout: it is the meeting.
    let opener = std::thread::spawn({
        let gate = gate.clone();
        move || {
            std::thread::sleep(Duration::from_millis(200));
            gate.open();
        }
    });
    assert!(transcript_final.wait(Duration::ZERO));
    opener.join().unwrap();

    assert_eq!(read(&transcript), BOTH_LINES);
    assert_eq!(notify.last_status().state, State::Stopped);
    assert!(
        !incomplete(&transcript),
        "marked complete once it ended cleanly"
    );
}

#[test]
fn a_late_batch_after_a_failed_recording_leaves_the_audio_marked() {
    // `finish` is the failed-recording path, which marks nothing itself.
    let opened = Arc::new(AtomicUsize::new(0));
    let gate = Gate::default();
    let (transcript, _live, notify, transcription) = record(
        "interrupted",
        batch(Batch::LinesAfter(gate.clone()), &opened),
    );

    let status = transcription.finish(Duration::from_millis(20));
    assert_ne!(status.state, State::Stopped);
    gate.open();
    wait_for("the batch to end", || {
        notify.last_status().state == State::Stopped
    });

    assert_eq!(
        read(&transcript),
        BOTH_LINES,
        "the transcript is still written"
    );
    assert!(incomplete(&transcript), "audio kept, as for live");
}

#[test]
fn an_engine_that_will_not_open_fails_in_a_sentence_and_is_final() {
    let open: OpenEngine = Box::new(|| Err("no speech engine is ready".to_string()));
    let (transcript, _live, _notify, transcription) = record("no-engine", open);

    let (status, mut transcript_final) = transcription.finish_final(Duration::from_secs(5));

    assert_eq!(status.state, State::Failed);
    let detail = status.detail.unwrap();
    assert!(detail.contains("no speech engine is ready"), "{detail}");
    assert!(
        detail.contains("The recording itself is complete"),
        "{detail}"
    );
    assert!(!detail.contains("Recording continues"), "{detail}");
    assert!(transcript_final.wait(Duration::ZERO));
    assert!(!transcript.exists());
    assert!(
        incomplete(&transcript),
        "a failed transcript keeps the audio"
    );
}

#[test]
fn an_engine_that_fails_on_the_audio_fails_the_status() {
    let opened = Arc::new(AtomicUsize::new(0));
    let (_transcript, _live, notify, transcription) = record("fails", batch(Batch::Fails, &opened));

    let (status, mut transcript_final) = transcription.finish_final(Duration::from_secs(5));

    assert_eq!(status.state, State::Failed);
    assert_eq!(status.engine.as_deref(), Some("batch-fake"));
    let detail = status.detail.unwrap();
    assert!(detail.contains("the fake engine fell over"), "{detail}");
    assert!(transcript_final.wait(Duration::ZERO));
    assert_eq!(notify.last_status().state, State::Failed);
}

#[test]
fn a_live_plan_still_opens_its_engine_while_recording() {
    // The default (`live: true`) is the path every other test here covers;
    // this pins that `Plan::live` and a bare `OpenEngine` both mean it.
    let opened = Arc::new(AtomicUsize::new(0));
    let transcript = temp_transcript("on");
    let live = LiveTranscript::default();
    let (_mic_tee, mic_feed) = audio::tee::tee();
    let transcription = live.start(
        Arc::new(CollectingNotify::default()),
        transcript.path.clone(),
        vec![(Speaker::You, mic_feed)],
        batch(Batch::Lines, &opened),
    );
    wait_for("the engine to open", || opened.load(Ordering::SeqCst) == 1);
    transcription.finish(Duration::from_secs(5));
}
