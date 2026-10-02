//! The board and the lifecycle, against the fake engine.

#![cfg(test)]

use std::time::{Duration, Instant};

use stt::{LiveLine, LiveUpdate, Speaker, SttEngine};

use super::*;

use super::fakes::*;

fn line(seq: u64, speaker: Speaker, text: &str) -> LiveLine {
    LiveLine {
        seq,
        speaker,
        start_sec: seq as f64,
        text: text.into(),
    }
}

// --- the board ------------------------------------------------------

#[test]
fn a_volatile_replaces_only_that_speakers_tail() {
    let mut lines = Lines::default();
    lines.apply(&LiveUpdate::Volatile(line(0, Speaker::You, "sess")));
    lines.apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "morn")));
    lines.apply(&LiveUpdate::Volatile(line(2, Speaker::You, "sessions are")));

    let volatile = lines.volatile();
    assert_eq!(volatile.len(), 2, "one tail per speaker, never a list");
    assert_eq!(volatile[0].text, "sessions are");
    assert_eq!(volatile[1].text, "morn");
    assert!(
        lines.finals.is_empty(),
        "a volatile is never a settled line"
    );
}

#[test]
fn a_final_clears_its_speakers_tail_and_appends() {
    let mut lines = Lines::default();
    lines.apply(&LiveUpdate::Volatile(line(0, Speaker::You, "sessions are")));
    lines.apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "morn")));
    lines.apply(&LiveUpdate::Final(line(
        2,
        Speaker::You,
        "Sessions are in memory.",
    )));

    assert_eq!(
        lines.finals,
        [line(2, Speaker::You, "Sessions are in memory.")]
    );
    let volatile = lines.volatile();
    assert_eq!(volatile.len(), 1, "the other speaker's tail is untouched");
    assert_eq!(volatile[0].speaker, Speaker::Others);
}

#[test]
fn a_dropped_clears_the_tail_and_appends_nothing() {
    let mut lines = Lines::default();
    lines.apply(&LiveUpdate::Volatile(line(0, Speaker::Others, "I")));
    lines.apply(&LiveUpdate::Dropped {
        speaker: Speaker::Others,
        seq: 1,
    });
    assert!(lines.volatile().is_empty());
    assert!(lines.finals.is_empty());
}

// --- the wire shapes the frontend is built against --------------------

#[test]
fn the_status_payload_has_the_agreed_shape() {
    let json = serde_json::to_value(Status {
        state: State::Failed,
        engine: Some("whisper".into()),
        detail: Some("It stopped.".into()),
    })
    .unwrap();
    assert_eq!(
        json,
        serde_json::json!({"state": "failed", "engine": "whisper", "detail": "It stopped."})
    );
    assert_eq!(
        serde_json::to_value(Status::idle()).unwrap(),
        serde_json::json!({"state": "idle", "engine": null, "detail": null})
    );
}

#[test]
fn the_snapshot_payload_has_the_agreed_shape() {
    let live = LiveTranscript::default();
    {
        let mut board = lock_or_recover(&live.board);
        board
            .lines
            .apply(&LiveUpdate::Final(line(0, Speaker::You, "Morning.")));
        board
            .lines
            .apply(&LiveUpdate::Volatile(line(1, Speaker::Others, "hi")));
    }
    let json = serde_json::to_value(live.snapshot()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "status": {"state": "idle", "engine": null, "detail": null},
            "finals": [{"seq": 0, "speaker": "you", "start_sec": 0.0, "text": "Morning."}],
            "volatile": [{"seq": 1, "speaker": "others", "start_sec": 1.0, "text": "hi"}],
        })
    );
}

// --- the lifecycle ----------------------------------------------------

#[test]
fn finals_reach_transcript_md_in_the_spec_format_and_the_window_sees_everything() {
    let path = temp_transcript("happy");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        fake(Mode::Echo),
    );
    mic_tee.offer(&[1; 160]);
    sys_tee.offer(&[1; 160]);
    mic_tee.offer(&[1; 160]);

    // Mid-meeting: both the file and the snapshot already have the lines —
    // nothing waits for Stop to be flushed.
    wait_for("three settled lines", || live.snapshot().finals.len() == 3);
    let mid = read(&path);
    assert!(
        mid.contains("You: You line 2."),
        "transcript.md so far: {mid:?}"
    );
    assert_eq!(live.snapshot().status.state, State::Running);
    assert_eq!(live.snapshot().status.engine.as_deref(), Some("fake"));

    // The recording stops: the tees go away, then transcription finishes.
    drop((mic_tee, sys_tee));
    let status = transcription.finish(STOP_TIMEOUT);
    assert_eq!(status.state, State::Stopped);
    assert_eq!(
        notify.states(),
        [State::Idle, State::Running, State::Stopped]
    );

    let body = read(&path);
    let mut lines: Vec<&str> = body.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "[00:00:01] Others: Others line 1.",
            "[00:00:01] You: You line 1.",
            "[00:00:02] You: You line 2.",
        ],
        "two tracks, one §3.4 file"
    );

    let updates = notify.updates.lock().unwrap().clone();
    let seqs: std::collections::HashSet<u64> = updates.iter().map(LiveUpdate::seq).collect();
    assert_eq!(
        seqs.len(),
        updates.len(),
        "seq is unique across both tracks"
    );
    // A volatile never reaches disk…
    assert!(!body.contains("hearing something") && !body.contains("and then"));
    // …and the guesses left showing at the end were withdrawn, not kept.
    assert!(
        live.snapshot().volatile.is_empty(),
        "no stale tail after Stop"
    );
    assert!(
        updates
            .iter()
            .any(|update| matches!(update, LiveUpdate::Dropped { .. }))
    );
}

#[test]
fn an_engine_that_will_not_start_fails_transcription_and_nothing_else() {
    let path = temp_transcript("no-engine");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        Box::new(|| Err("no speech engine is ready".to_string())),
    );
    wait_for("the failure", || {
        live.snapshot().status.state == State::Failed
    });

    let detail = live.snapshot().status.detail.unwrap();
    assert!(detail.contains("no speech engine is ready"), "{detail}");
    assert!(detail.contains("Recording continues"), "{detail}");

    // The capture side keeps offering audio into a tee nobody reads any
    // more. That must cost it nothing: no block, no panic, no error.
    for _ in 0..(audio::tee::DEFAULT_CAPACITY_CHUNKS + 10) {
        mic_tee.offer(&[1; 160]);
    }

    let started = Instant::now();
    let status = transcription.finish(STOP_TIMEOUT);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "Stop did not wait"
    );
    assert_eq!(
        status.state,
        State::Failed,
        "the failure stays the last word"
    );
    assert_eq!(read(&path), "", "nothing was written");
}

#[test]
fn a_mid_meeting_failure_stops_both_tracks_and_keeps_what_settled() {
    let path = temp_transcript("mid-failure");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        fake(Mode::FailOnFeed(2)),
    );
    mic_tee.offer(&[1; 160]);
    wait_for("the first line", || !live.snapshot().finals.is_empty());
    mic_tee.offer(&[1; 160]); // the one that fails
    wait_for("the failure", || {
        live.snapshot().status.state == State::Failed
    });

    let status = live.snapshot().status;
    let detail = status.detail.unwrap();
    assert!(detail.contains("your microphone"), "{detail}");
    assert!(detail.contains("the fake engine fell over"), "{detail}");
    assert_eq!(status.engine.as_deref(), Some("fake"));

    // The recording is still going; the other track's audio has nowhere
    // to go now, and that is fine.
    sys_tee.offer(&[1; 160]);
    mic_tee.offer(&[1; 160]);

    drop((mic_tee, sys_tee));
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
    assert!(
        read(&path).contains("[00:00:01] You: You line 1."),
        "what settled before the failure is kept"
    );
    assert!(
        live.snapshot().volatile.is_empty(),
        "the failed track's guess was withdrawn, not left on screen"
    );
}

#[test]
fn a_wedged_engine_cannot_hold_stop_hostage_or_touch_the_next_meeting() {
    let path = temp_transcript("wedged");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::WedgeOnFinish),
    );
    mic_tee.offer(&[1; 160]);
    wait_for("the first line", || !live.snapshot().finals.is_empty());
    drop(mic_tee);

    let started = Instant::now();
    let status = transcription.finish(Duration::from_millis(200));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "Stop was bounded"
    );
    assert_eq!(status.state, State::Failed);
    assert!(status.detail.unwrap().contains("did not finish"));
    assert!(
        read(&path).contains("You: You line 1."),
        "lines that settled before Stop are already on disk"
    );

    // The next meeting starts while the old engine thread is still stuck.
    let (_tee, feed) = audio::tee::tee();
    let next = live.start(
        notify.clone(),
        temp_transcript("wedged-next"),
        vec![(Speaker::You, feed)],
        Box::new(|| Err("not this time".to_string())),
    );
    assert!(
        live.snapshot().finals.is_empty(),
        "a new meeting starts clean"
    );
    let _ = next.finish(STOP_TIMEOUT);
}

#[test]
fn a_sealed_meeting_ignores_late_writes() {
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let scope = Scope {
        board: Arc::clone(&live.board),
        generation: lock_or_recover(&live.board).generation,
        notify: notify.clone(),
    };
    scope.update(&LiveUpdate::Final(line(0, Speaker::You, "In time.")));
    scope.seal();
    scope.update(&LiveUpdate::Final(line(1, Speaker::You, "Too late.")));
    scope.fail("late".into());

    let snapshot = live.snapshot();
    assert_eq!(snapshot.finals, [line(0, Speaker::You, "In time.")]);
    assert_eq!(snapshot.status.state, State::Idle);
    assert_eq!(
        notify.updates.lock().unwrap().len(),
        1,
        "nothing late was emitted"
    );
}

#[test]
fn a_failure_is_not_overwritten_by_a_later_clean_finish() {
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let scope = Scope {
        board: Arc::clone(&live.board),
        generation: lock_or_recover(&live.board).generation,
        notify: notify.clone(),
    };
    scope.running("fake");
    scope.fail("first".into());
    scope.fail("second".into());
    scope.running("fake");
    scope.settle();

    assert_eq!(
        notify.states(),
        [State::Running, State::Failed, State::Failed],
        "one failure reported, then re-sent as the final word"
    );
    assert_eq!(notify.last_status().detail.as_deref(), Some("first"));
}

// --- adversarial ------------------------------------------------------

/// One chunk of audio. The fake engine settles one line per chunk, so
/// the content never matters, only the count.
fn chunk() -> Vec<i16> {
    vec![1; 160]
}

fn final_texts(live: &LiveTranscript) -> Vec<String> {
    live.snapshot()
        .finals
        .into_iter()
        .map(|line| line.text)
        .collect()
}

fn finals_of(updates: &[LiveUpdate]) -> impl Iterator<Item = u64> + '_ {
    updates.iter().filter_map(|update| match update {
        LiveUpdate::Final(line) => Some(line.seq),
        _ => None,
    })
}

#[test]
fn an_engine_that_panics_mid_meeting_is_reported_while_the_meeting_is_still_on() {
    let path = temp_transcript("panic");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        fake(Mode::PanicOnFeed(2)),
    );
    mic_tee.offer(&chunk());
    wait_for("the first line", || !live.snapshot().finals.is_empty());
    mic_tee.offer(&chunk()); // the one that panics

    // The window has to hear about it now, not at Stop: the user is
    // still in the meeting, looking at a pane that has gone quiet.
    wait_for("the panic to be reported", || {
        live.snapshot().status.state == State::Failed
    });
    let detail = live.snapshot().status.detail.unwrap();
    assert!(detail.contains("your microphone"), "{detail}");
    assert!(detail.contains("crashed"), "{detail}");
    assert!(detail.contains("Recording continues"), "{detail}");

    sys_tee.offer(&chunk());
    drop((mic_tee, sys_tee));
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
    assert!(read(&path).contains("You: You line 1."));
    assert!(
        live.snapshot().volatile.is_empty(),
        "the crashed track's guess was still withdrawn"
    );
}

#[test]
fn a_track_whose_session_will_not_open_fails_without_leaving_the_other_running() {
    let path = temp_transcript("no-session");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        fake(Mode::FailSessionFor(Speaker::Others)),
    );
    wait_for("the failure", || {
        live.snapshot().status.state == State::Failed
    });
    let status = live.snapshot().status;
    assert!(
        status
            .detail
            .as_deref()
            .unwrap()
            .contains("the other people on the call"),
        "{status:?}"
    );
    assert_eq!(status.engine.as_deref(), Some("fake"));

    // Capture carries on into tees nobody reads.
    for _ in 0..(audio::tee::DEFAULT_CAPACITY_CHUNKS + 10) {
        mic_tee.offer(&chunk());
        sys_tee.offer(&chunk());
    }
    drop((mic_tee, sys_tee));
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Failed);
    assert_eq!(read(&path), "");
    assert!(
        !notify.states().contains(&State::Running),
        "never claimed to be transcribing"
    );
}

#[test]
fn a_microphone_only_meeting_transcribes_the_microphone() {
    let path = temp_transcript("mic-only");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();

    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::Echo),
    );
    mic_tee.offer(&chunk());
    mic_tee.offer(&chunk());
    drop(mic_tee);
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
    assert_eq!(
        read(&path),
        "[00:00:01] You: You line 1.\n[00:00:02] You: You line 2.\n"
    );
}

#[test]
fn stop_before_the_engine_has_loaded_still_transcribes_what_was_recorded() {
    let path = temp_transcript("slow-load");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();

    // A model that takes a while to load; the whole meeting is over
    // before it has.
    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        Box::new(|| {
            std::thread::sleep(Duration::from_millis(300));
            Ok(Box::new(FakeEngine(Mode::Echo)) as Box<dyn SttEngine>)
        }),
    );
    for _ in 0..3 {
        mic_tee.offer(&chunk());
    }
    drop(mic_tee);
    assert_eq!(live.snapshot().status.state, State::Idle, "still loading");

    let status = transcription.finish(STOP_TIMEOUT);
    assert_eq!(status.state, State::Stopped);
    assert_eq!(
        read(&path).lines().count(),
        3,
        "the tee held the audio until the engine was ready"
    );
    assert_eq!(
        notify.states(),
        [State::Idle, State::Running, State::Stopped]
    );
}

#[test]
fn an_engine_that_comes_back_after_stop_gave_up_cannot_reach_the_next_meeting() {
    let first_path = temp_transcript("late-first");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let gate = Gate::default();
    let (mic_tee, mic_feed) = audio::tee::tee();

    // The first meeting writes to the same window, through a handle of its
    // own, so the test can tell when it has let go of it.
    let first_window: Arc<dyn Notify> = Arc::new(Forward(notify.clone()));
    let first_alive = Arc::downgrade(&first_window);
    let first = live.start(
        first_window,
        first_path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::WedgeOnFeed(gate.clone())),
    );
    mic_tee.offer(&chunk());
    drop(mic_tee);
    assert_eq!(
        first.finish(Duration::from_millis(200)).state,
        State::Failed
    );

    // The next meeting is running when the first one's engine wakes up.
    let second_path = temp_transcript("late-second");
    let (mic_tee, mic_feed) = audio::tee::tee();
    let second = live.start(
        notify.clone(),
        second_path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::Echo),
    );
    mic_tee.offer(&chunk());
    // The echo answers with a guess, the line and a trailing guess. Wait
    // for all three, not just the line, so none of the second meeting's own
    // updates is still on its way when the count is taken.
    wait_for("the second meeting's line and trailing guess", || {
        matches!(
            notify.updates.lock().unwrap().as_slice(),
            [
                LiveUpdate::Volatile(_),
                LiveUpdate::Final(_),
                LiveUpdate::Volatile(_)
            ]
        )
    });
    let updates_before = notify.updates.lock().unwrap().len();

    gate.open();
    // Let the woken engine run to the end: settle its line, finish, and
    // have its threads exit. Once the last of them has dropped its handle
    // on the window, nothing of the first meeting can write any more.
    wait_for("the first meeting's threads to end", || {
        first_alive.strong_count() == 0
    });

    assert_eq!(final_texts(&live), ["You line 1."]);
    assert_eq!(
        notify.updates.lock().unwrap().len(),
        updates_before,
        "the first meeting's late line was never sent to the window"
    );
    assert_eq!(live.snapshot().status.state, State::Running);
    assert_eq!(read(&second_path), "[00:00:01] You: You line 1.\n");

    drop(mic_tee);
    assert_eq!(second.finish(STOP_TIMEOUT).state, State::Stopped);
}

#[test]
fn rapid_start_stop_cycles_keep_every_meeting_to_itself() {
    let live = LiveTranscript::default();
    for round in 0..25 {
        let path = temp_transcript(&format!("rapid-{round}"));
        let notify = Arc::new(CollectingNotify::default());
        let (mic_tee, mic_feed) = audio::tee::tee();
        let (sys_tee, sys_feed) = audio::tee::tee();
        let transcription = live.start(
            notify.clone(),
            path.clone(),
            vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
            fake(Mode::Echo),
        );
        assert!(
            live.snapshot().finals.is_empty(),
            "round {round} starts clean"
        );
        // Some rounds stop before any audio, some with a little.
        for _ in 0..(round % 3) {
            mic_tee.offer(&chunk());
            sys_tee.offer(&chunk());
        }
        drop((mic_tee, sys_tee));
        let status = transcription.finish(STOP_TIMEOUT);
        assert_eq!(status.state, State::Stopped, "round {round}");
        let expected = 2 * (round % 3);
        assert_eq!(live.snapshot().finals.len(), expected, "round {round}");
        assert_eq!(read(&path).lines().count(), expected, "round {round}");
        let seqs: Vec<u64> = live.snapshot().finals.iter().map(|l| l.seq).collect();
        assert!(
            seqs.iter().all(|&seq| seq < 20),
            "seq restarts with each meeting: {seqs:?}"
        );
    }
}

#[test]
fn a_snapshot_taken_mid_stream_plus_the_events_after_it_is_every_line_once() {
    let path = temp_transcript("snapshot-race");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();
    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed), (Speaker::Others, sys_feed)],
        fake(Mode::Echo),
    );

    let producer = std::thread::spawn(move || {
        for _ in 0..400 {
            mic_tee.offer(&chunk());
            sys_tee.offer(&chunk());
            std::thread::yield_now();
        }
    });
    // A window opening at an arbitrary moment: it subscribes (so every
    // event from `from` on reaches it), then asks for the snapshot.
    let mut windows = Vec::new();
    while !producer.is_finished() {
        let from = notify.updates.lock().unwrap().len();
        windows.push((from, live.snapshot()));
    }
    producer.join().unwrap();
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);

    let updates = notify.updates.lock().unwrap().clone();
    let every: std::collections::BTreeSet<u64> = finals_of(&updates).collect();
    assert_eq!(every.len(), 800);
    assert!(windows.len() > 1, "the race was actually exercised");
    for (from, snapshot) in windows {
        let mut seen: std::collections::BTreeSet<u64> =
            snapshot.finals.iter().map(|line| line.seq).collect();
        seen.extend(finals_of(&updates[from..]));
        assert_eq!(
            seen, every,
            "a window that opened at event {from} missed lines"
        );
    }
    assert_eq!(read(&path).lines().count(), 800);
}

#[test]
fn a_long_meeting_keeps_every_line_and_nothing_else() {
    // A few hours' worth of lines. The board grows by lines, never by
    // audio, and the pane and the file end up agreeing on all of them.
    let path = temp_transcript("long");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::Echo),
    );
    for offered in 0..3000 {
        // Paced to the consumer so the tee never has to drop (that path
        // has its own tests); three updates per chunk from `Echo`.
        while offered > notify.updates.lock().unwrap().len() / 3 + 500 {
            std::thread::yield_now();
        }
        mic_tee.offer(&chunk());
    }
    drop(mic_tee);
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
    let snapshot = live.snapshot();
    assert!(snapshot.volatile.is_empty());
    assert_eq!(snapshot.finals.len(), 3000);
    assert_eq!(read(&path).lines().count(), 3000);
}

#[test]
fn a_guess_that_is_never_settled_or_withdrawn_does_not_stay_on_screen() {
    // Apple's model does this over room tone: one "I", then nothing. It
    // must not sit in the pane as someone "still speaking" for the rest
    // of a quiet stretch — the live form of the hallucination bug.
    let path = temp_transcript("stale-guess");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let feeds = Feeds::default();
    let (mic_tee, mic_feed) = audio::tee::tee();
    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::GuessOnce(feeds.clone())),
    );
    let second = vec![0; 16_000];
    mic_tee.offer(&second);
    wait_for("the guess", || !live.snapshot().volatile.is_empty());

    // Still up three seconds of audio later: a guess may be a guess. The
    // fifth second reaching the engine means the fourth has been fed and
    // checked; the fifth's own check cannot withdraw it either.
    for _ in 0..4 {
        mic_tee.offer(&second);
    }
    wait_for("the fifth second to reach the engine", || {
        feeds.started() == 5
    });
    assert!(
        !live.snapshot().volatile.is_empty(),
        "withdrawn too eagerly"
    );

    for _ in 1..STALE_GUESS.as_secs() {
        mic_tee.offer(&second);
    }
    // The board is cleared under its lock and the window told just after,
    // so wait on the window: once it has the `Dropped`, the board is clear.
    wait_for("the window to be told to clear the stale guess", || {
        matches!(
            notify.updates.lock().unwrap().last(),
            Some(LiveUpdate::Dropped {
                speaker: Speaker::You,
                ..
            })
        )
    });
    assert!(
        live.snapshot().volatile.is_empty(),
        "the stale guess is off the board too"
    );
    let updates = notify.updates.lock().unwrap().clone();
    let seqs: Vec<u64> = updates.iter().map(LiveUpdate::seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");

    drop(mic_tee);
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
    assert_eq!(read(&path), "", "a guess never reaches the file");
}

#[test]
fn a_guess_the_engine_keeps_updating_is_never_withdrawn() {
    // A long monologue: the guess changes with every chunk, so however
    // long it runs it is live, not stale.
    let path = temp_transcript("fresh-guess");
    let live = LiveTranscript::default();
    let notify = Arc::new(CollectingNotify::default());
    let (mic_tee, mic_feed) = audio::tee::tee();
    let transcription = live.start(
        notify.clone(),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        fake(Mode::Echo),
    );
    for _ in 0..(STALE_GUESS.as_secs() * 3) {
        mic_tee.offer(&vec![0; 16_000]);
    }
    drop(mic_tee);
    assert_eq!(transcription.finish(STOP_TIMEOUT).state, State::Stopped);
    let dropped = notify
        .updates
        .lock()
        .unwrap()
        .iter()
        .filter(|u| matches!(u, LiveUpdate::Dropped { .. }))
        .count();
    assert_eq!(dropped, 1, "only the one `finish` sends");
}

// --- when transcript.md is final (TUR-17) -----------------------------

/// How long a [`LateLineEngine`] takes inside `finish`.
#[derive(Clone)]
enum Hold {
    For(Duration),
    Until(Gate),
}

/// An engine that settles one more line inside `finish`, after its [`Hold`]:
/// a slow engine flushing its tail, possibly after Stop stopped waiting.
struct LateLineEngine(Hold);

impl SttEngine for LateLineEngine {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn transcribe(
        &mut self,
        _wav: &std::path::Path,
        _speaker: Speaker,
        _sink: &mut dyn stt::TranscriptSink,
    ) -> Result<(), stt::Error> {
        Ok(())
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start_session(
        &mut self,
        options: stt::SessionOptions,
        sink: Box<dyn stt::TranscriptSink + Send>,
        listener: Box<dyn stt::LiveListener>,
    ) -> Result<Box<dyn stt::SttSession>, stt::Error> {
        Ok(Box::new(LateLineSession {
            speaker: options.speaker,
            emitter: stt::LiveEmitter::new(&options, listener),
            sink,
            hold: self.0.clone(),
        }))
    }
}

struct LateLineSession {
    speaker: Speaker,
    emitter: stt::LiveEmitter,
    sink: Box<dyn stt::TranscriptSink + Send>,
    hold: Hold,
}

impl stt::SttSession for LateLineSession {
    fn engine_name(&self) -> &'static str {
        "fake"
    }

    fn feed(&mut self, _samples: &[i16]) -> Result<(), stt::Error> {
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<stt::SessionOutcome, stt::Error> {
        match &self.hold {
            Hold::For(delay) => std::thread::sleep(*delay),
            Hold::Until(gate) => gate.wait(),
        }
        self.emitter
            .finalize(1.0, "The last line.", self.sink.as_mut())?;
        self.sink.flush()?;
        Ok(stt::SessionOutcome {
            speaker: self.speaker,
            finalized: self.emitter.finalized(),
            discarded_volatile: self.emitter.withdraw(),
            audio_sec: 0,
            engine: "fake",
        })
    }
}

/// Start a microphone-only meeting on a [`LateLineEngine`] and stop the
/// recording at once, so the engine's `finish` is all that is left.
fn stop_a_late_line_meeting(
    name: &str,
    hold: Hold,
    timeout: Duration,
) -> (PathBuf, Status, TranscriptFinal) {
    let path = temp_transcript(name);
    let live = LiveTranscript::default();
    let (mic_tee, mic_feed) = audio::tee::tee();
    let transcription = live.start(
        Arc::new(CollectingNotify::default()),
        path.clone(),
        vec![(Speaker::You, mic_feed)],
        Box::new(move || Ok(Box::new(LateLineEngine(hold)) as Box<dyn SttEngine>)),
    );
    drop(mic_tee);
    let (status, transcript_final) = transcription.finish_final(timeout);
    (path, status, transcript_final)
}

const LAST_LINE: &str = "[00:00:01] You: The last line.";

#[test]
fn an_engine_that_finishes_in_time_leaves_transcript_md_final_at_once() {
    let (path, status, mut transcript_final) =
        stop_a_late_line_meeting("final-in-time", Hold::For(Duration::ZERO), STOP_TIMEOUT);
    assert_eq!(status.state, State::Stopped);
    assert!(
        transcript_final.wait(Duration::ZERO),
        "nothing left to wait for"
    );
    assert!(read(&path).contains(LAST_LINE), "{:?}", read(&path));
}

#[test]
fn an_engine_slower_than_stop_makes_transcript_md_final_only_once_it_ends() {
    let (path, status, mut transcript_final) = stop_a_late_line_meeting(
        "final-late",
        Hold::For(Duration::from_millis(300)),
        Duration::from_millis(50),
    );
    assert_eq!(status.state, State::Failed, "Stop gave up on the engine");
    assert!(
        !transcript_final.wait(Duration::ZERO),
        "the engine is still finishing"
    );
    assert!(
        !read(&path).contains(LAST_LINE),
        "the last line is not written yet"
    );

    assert!(
        transcript_final.wait(Duration::from_secs(5)),
        "a later wait sees the engine end"
    );
    assert!(
        read(&path).contains(LAST_LINE),
        "final means the late line is in: {:?}",
        read(&path)
    );
    assert!(transcript_final.wait(Duration::ZERO), "and it stays final");
}

#[test]
fn an_engine_that_never_finishes_never_makes_transcript_md_final() {
    let gate = Gate::default();
    let (path, status, mut transcript_final) = stop_a_late_line_meeting(
        "final-never",
        Hold::Until(gate.clone()),
        Duration::from_millis(50),
    );
    assert_eq!(status.state, State::Failed);
    assert!(!transcript_final.wait(Duration::from_millis(100)));
    assert!(
        !transcript_final.wait(Duration::from_millis(100)),
        "still not final"
    );
    assert!(!read(&path).contains(LAST_LINE));

    // Let the engine go, so its thread does not outlive the test.
    gate.open();
    assert!(transcript_final.wait(Duration::from_secs(5)));
}
