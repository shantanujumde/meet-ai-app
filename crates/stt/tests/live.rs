//! The silence gate and the tail contract, on the *streaming* path.
//!
//! `silence.rs` proves that a finished recording of 30 quiet seconds produces
//! zero lines. This file proves the same thing for audio that arrives a block
//! at a time, plus the half that only exists live: **no stale volatile tail**.
//! A session that shows a guess and then goes quiet forever leaves that guess
//! on screen, which is the streaming-shaped version of "Thank you." over
//! silence and is just as wrong.
//!
//! Everything here runs against the real fixture WAVs and the real detector.
//! The whisper half is opt-in behind `whisper-model-tests` because it needs a
//! 190 MB model; the replay half needs nothing and runs in `just check`.

// Every OS (TUR-50): the fixture WAVs are committed. The Apple tests skip
// themselves, with a SKIPPED line, where the meet-stt sidecar is not built,
// which is everywhere but a Mac.

mod fixtures;

use stt::replay::{ReplayEngine, ReplayOptions, replay_samples, replay_track};
use stt::session::{CollectingListener, SharedCollector};
use stt::{SeqCounter, SessionOptions, Speaker, SttEngine};

/// The 30-second gate, live, on both quiet fixtures.
///
/// `room-tone-30s.wav` is the one that matters: digital silence passes any
/// detector, while quiet pink noise is what a too-permissive threshold opens a
/// span on.
#[test]
fn thirty_seconds_of_quiet_streams_no_lines_and_no_tail() {
    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let pcm = stt::read_wav_16k_mono(&fixtures::path(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut engine = ReplayEngine::new();
        let mut session = engine
            .start_session(
                SessionOptions::new(Speaker::You),
                Box::new(sink.clone()),
                Box::new(seen.clone()),
            )
            .expect("the replay engine streams");

        replay_samples(&pcm, session.as_mut(), &ReplayOptions::instant()).unwrap();
        let outcome = session.finish().unwrap();

        assert_eq!(
            outcome.finalized,
            0,
            "{name}: {} line(s) invented over quiet audio: {:?}",
            outcome.finalized,
            sink.lines()
        );
        assert!(sink.is_empty(), "{name}: quiet audio reached disk");
        assert!(
            seen.volatiles().is_empty(),
            "{name}: a hypothesis was shown for audio with no speech in it: {:?}",
            seen.volatiles()
        );
        assert_eq!(
            seen.tail_for(Speaker::You),
            None,
            "{name}: a stale tail was left on screen"
        );
        assert_eq!(outcome.audio_sec, 30);
    }
}

/// A meeting that is quiet all the way to the end leaves nothing behind.
///
/// The `finish()` half of the gate: even with both tracks running, the pane
/// must end with no live line for either speaker.
#[test]
fn a_silent_two_track_meeting_ends_with_an_empty_pane() {
    let seq = SeqCounter::new();
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();

    for (speaker, name) in [
        (Speaker::You, "silence-30s.wav"),
        (Speaker::Others, "room-tone-30s.wav"),
    ] {
        let mut engine = ReplayEngine::new();
        let outcome = replay_track(
            &fixtures::path(name),
            &mut engine,
            SessionOptions::new(speaker).with_seq(seq.clone()),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
            &ReplayOptions::instant(),
        )
        .unwrap();

        assert_eq!(outcome.finalized, 0);
        assert!(!outcome.discarded_volatile, "there was nothing to discard");
    }

    assert!(sink.is_empty(), "transcript: {:?}", sink.lines());
    assert!(
        seen.updates().is_empty(),
        "the pane was told nothing at all"
    );
    assert_eq!(seq.issued(), 0, "no update, no sequence number");
    assert_eq!(seen.tail_for(Speaker::You), None);
    assert_eq!(seen.tail_for(Speaker::Others), None);
}

/// The tail contract, against real speech.
///
/// Three properties the Phase 2 pane relies on, asserted together because they
/// are only meaningful together: one live line per speaker, `seq` unique and
/// increasing across *both* speakers, and nothing volatile on disk.
#[test]
fn the_two_track_fixture_honours_the_tail_contract() {
    let seq = SeqCounter::new();
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();

    for (speaker, name) in [
        (Speaker::You, "two-speaker-60s/mic.wav"),
        (Speaker::Others, "two-speaker-60s/system.wav"),
    ] {
        let mut engine = ReplayEngine::new();
        // Uncapped, so the assertions cannot depend on how fast the machine
        // ran the test. The rate cap has its own unit test.
        replay_track(
            &fixtures::path(name),
            &mut engine,
            SessionOptions::new(speaker)
                .with_seq(seq.clone())
                .with_volatile_per_sec(f64::INFINITY),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
            &ReplayOptions::instant(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    let updates = seen.updates();
    assert!(!updates.is_empty(), "the speech fixture produced nothing");

    let seqs: Vec<u64> = updates.iter().map(stt::LiveUpdate::seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        seqs.len(),
        sorted.len(),
        "two speakers collided on a sequence number, so the pane's React keys \
         are not unique"
    );

    assert_eq!(seen.tail_for(Speaker::You), None, "You kept a live line");
    assert_eq!(
        seen.tail_for(Speaker::Others),
        None,
        "Others kept a live line"
    );

    // Volatiles outnumber finals, and none of them reached the sink.
    let finals = seen.finals();
    assert!(seen.volatiles().len() >= finals.len());
    assert_eq!(
        sink.len(),
        finals.len(),
        "every settled line reached disk exactly once, and nothing else did"
    );
    for (utterance, line) in sink.utterances().iter().zip(&finals) {
        assert_eq!(utterance.text, line.text);
    }
}

/// Live and batch must not disagree.
///
/// If they did, a line would visibly change between the pane and the saved
/// `transcript.md`, which is a bug the user notices and cannot explain.
#[test]
fn streaming_a_track_gives_the_same_lines_as_transcribing_it() {
    let wav = fixtures::path("two-speaker-60s/mic.wav");

    let live = SharedCollector::new();
    let mut engine = ReplayEngine::new();
    replay_track(
        &wav,
        &mut engine,
        SessionOptions::new(Speaker::You),
        Box::new(live.clone()),
        Box::new(stt::NoListener),
        &ReplayOptions::instant(),
    )
    .unwrap();

    let mut batch = stt::CollectingSink::new();
    engine.transcribe(&wav, Speaker::You, &mut batch).unwrap();

    assert!(!batch.utterances.is_empty());
    assert_eq!(live.lines(), batch.lines());
}

/// The Apple engine's live path, driven for real through `meet-stt --stdin`.
///
/// TUR-31 decided stdin, per channel, two sidecars. TUR-33 wires it onto the
/// same [`stt::session::SttSession`] seam every other engine uses, so this is
/// deliberately the whisper-shaped test, not a bespoke one: same
/// `start_session`, same [`stt::replay::replay_samples`], same assertions.
///
/// Needs a built sidecar and macOS 26+ with the locale installed, so it skips
/// loudly rather than failing on a machine that has neither — same pattern as
/// `tests/sidecar.rs`.
#[test]
fn the_apple_engine_reports_that_it_streams() {
    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };

    let engine = stt::apple::AppleEngine::new(binary, "en-US");
    assert!(
        engine.supports_streaming(),
        "AppleEngine has a real stdin streaming path (TUR-31/TUR-33) and must say so"
    );
}

/// The silence gate, on the Apple engine, over a real subprocess.
///
/// Same assertions as `whisper_streams_nothing_over_thirty_quiet_seconds`.
/// The gate sits in a different place, though. Unlike whisper and replay, the
/// Apple engine has no [`stt::session::SpanAssembler`] in front of it. SPEC
/// §2.5 is explicit that engine 1 does native long-form streaming precisely so
/// it does *not* need VAD chunking, so the analyzer hears the quiet audio too.
/// Measured on this machine (macOS 27.0, `en-US` installed):
/// `silence-30s.wav` produces nothing, but `room-tone-30s.wav` makes Apple
/// guess `"I"`. In about half of all pink-noise draws the guess is also
/// *finalized*. The fixture's noise is seeded (`seed=1` in `generate.sh`) to
/// one of those draws: the raw sidecar settles `"I"` over its first few
/// seconds, so this test exercises the gate on every run instead of on a coin
/// flip. What stops it is [`stt::vad::SpeechTimeline`]: every Apple result is
/// held against the
/// same detector whisper is gated by, and one over audio the detector heard no
/// speech in is neither shown nor written.
#[test]
fn apple_streams_nothing_settled_over_thirty_quiet_seconds() {
    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };

    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let pcm = stt::read_wav_16k_mono(&fixtures::path(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        let sink = SharedCollector::new();
        let seen = CollectingListener::new();
        let mut engine = stt::apple::AppleEngine::new(binary.clone(), "en-US");
        let mut session = match engine.start_session(
            SessionOptions::new(Speaker::You),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
        ) {
            Ok(session) => session,
            Err(e) => {
                eprintln!(
                    "SKIPPED: could not start a live apple session ({e}) — is en-US installed?"
                );
                return;
            }
        };

        replay_samples(&pcm, session.as_mut(), &ReplayOptions::instant())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let outcome = session.finish().unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(
            outcome.finalized,
            0,
            "{name}: {} line(s) settled over quiet audio: {:?}",
            outcome.finalized,
            sink.lines()
        );
        assert!(sink.is_empty(), "{name}: quiet audio reached disk");
        assert!(
            seen.volatiles().is_empty(),
            "{name}: a hypothesis was shown for audio with no speech in it: {:?}",
            seen.volatiles()
        );
        assert_eq!(
            seen.tail_for(Speaker::You),
            None,
            "{name}: a stale tail was left on screen after finish()"
        );
        assert_eq!(
            outcome.audio_sec, 30,
            "{name}: audio_sec did not match feed"
        );
    }
}

/// Apple live vs Apple batch must not disagree, the way the whisper and replay
/// equivalents do not — otherwise the pane would show one thing during the
/// meeting and `transcript.md` would show another.
///
/// Real subprocess, real hardware, real speech: `meet-stt --stdin` streamed a
/// block at a time against the same fixture `meet-stt <wav>` transcribes in
/// batch.
#[test]
fn apple_streams_the_same_lines_it_batches() {
    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };

    let wav = fixtures::path("two-speaker-60s/mic.wav");
    let pcm = stt::read_wav_16k_mono(&wav).unwrap();

    let live = SharedCollector::new();
    let mut engine = stt::apple::AppleEngine::new(binary, "en-US");
    let mut session = match engine.start_session(
        SessionOptions::new(Speaker::You),
        Box::new(live.clone()),
        Box::new(stt::NoListener),
    ) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("SKIPPED: could not start a live apple session ({e}) — is en-US installed?");
            return;
        }
    };
    replay_samples(&pcm, session.as_mut(), &ReplayOptions::instant()).unwrap();
    session.finish().unwrap();

    let mut batch = stt::CollectingSink::new();
    engine.transcribe(&wav, Speaker::You, &mut batch).unwrap();

    eprintln!("--- apple, streamed a block at a time ---");
    for line in live.lines() {
        eprintln!("{line}");
    }

    assert!(!batch.utterances.is_empty(), "the fixture has speech in it");
    assert_eq!(
        live.lines(),
        batch.lines(),
        "the live pane and transcript.md would show different text"
    );
}

/// The gate on the engine that actually hallucinates.
#[cfg(all(target_os = "macos", feature = "whisper-model-tests"))]
#[test]
fn whisper_streams_nothing_over_thirty_quiet_seconds() {
    use stt::whisper::{WhisperConfig, WhisperEngine};

    let Some(model) = fixtures::whisper_model() else {
        panic!("whisper-model-tests is on but no model is present; run `just model`");
    };

    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let pcm = stt::read_wav_16k_mono(&fixtures::path(name)).unwrap();
        let sink = SharedCollector::new();
        let seen = CollectingListener::new();

        // Partials on, which is the setting most likely to invent something:
        // it runs whisper over spans that have not settled yet.
        let config = WhisperConfig {
            live_partials: true,
            ..WhisperConfig::default()
        };
        let mut engine = WhisperEngine::load(&model, config).unwrap();
        let mut session = engine
            .start_session(
                SessionOptions::new(Speaker::You),
                Box::new(sink.clone()),
                Box::new(seen.clone()),
            )
            .unwrap();

        replay_samples(&pcm, session.as_mut(), &ReplayOptions::instant()).unwrap();
        let outcome = session.finish().unwrap();

        assert_eq!(
            outcome.finalized,
            0,
            "{name}: whisper hallucinated {} line(s) on the streaming path — \
             the exact Phase 1 failure the VAD gate exists to prevent: {:?}",
            outcome.finalized,
            sink.lines()
        );
        assert!(
            seen.volatiles().is_empty(),
            "{name}: whisper guessed at audio with no speech in it: {:?}",
            seen.volatiles()
        );
        assert_eq!(seen.tail_for(Speaker::You), None, "{name}: stale tail");
    }
}

/// Whisper live vs whisper batch, on real speech.
#[cfg(all(target_os = "macos", feature = "whisper-model-tests"))]
#[test]
fn whisper_streams_the_same_lines_it_batches() {
    use stt::whisper::{WhisperConfig, WhisperEngine};

    let Some(model) = fixtures::whisper_model() else {
        panic!("whisper-model-tests is on but no model is present; run `just model`");
    };
    let wav = fixtures::path("two-speaker-60s/mic.wav");
    let pcm = stt::read_wav_16k_mono(&wav).unwrap();

    let live = SharedCollector::new();
    let mut engine = WhisperEngine::load(&model, WhisperConfig::default()).unwrap();
    let mut session = engine
        .start_session(
            SessionOptions::new(Speaker::You),
            Box::new(live.clone()),
            Box::new(stt::NoListener),
        )
        .unwrap();
    replay_samples(&pcm, session.as_mut(), &ReplayOptions::instant()).unwrap();
    session.finish().unwrap();

    let mut batch = stt::CollectingSink::new();
    engine.transcribe(&wav, Speaker::You, &mut batch).unwrap();

    // Printed so the gate is readable in a log rather than only pass/fail —
    // `accuracy.rs` does the same. An empty-vs-empty comparison would pass
    // vacuously, so the non-empty assertion comes first.
    eprintln!("--- whisper, streamed a block at a time ---");
    for line in live.lines() {
        eprintln!("{line}");
    }

    assert!(!batch.utterances.is_empty(), "the fixture has speech in it");
    assert_eq!(
        live.lines(),
        batch.lines(),
        "the live pane and transcript.md would show different text"
    );
}
