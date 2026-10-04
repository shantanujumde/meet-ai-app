//! The hallucination guard, as a regression test.
//!
//! SPEC §5 Phase 1 exit gate: **30 seconds of silence produces zero transcript
//! lines**, on both engines. Whisper emitting "Thank you." over a quiet stretch
//! is the canonical failure of this component, so it is asserted here rather
//! than eyeballed once — that is the whole point of this file existing.
//!
//! Two fixtures, because they fail differently:
//!
//! * `silence-30s.wav` — digitally perfect zeroes. Any VAD passes this.
//! * `room-tone-30s.wav` — 30 s of quiet pink noise. This is the one that
//!   actually catches a too-permissive threshold, and it is the closer analogue
//!   of a real meeting room with an air conditioner in it.
//!
//! The whisper half is behind the `whisper-model-tests` feature because it
//! needs a 190 MB download. `just check` runs the Apple half and the pure-VAD
//! half, which need nothing.

// Every OS (TUR-50): the fixture WAVs are committed. The Apple tests skip
// themselves, with a SKIPPED line, where the meet-stt sidecar is not built,
// which is everywhere but a Mac.

mod fixtures;

use stt::sink::CollectingSink;
use stt::vad::{EarshotVad, SegmentConfig, detect_speech};
use stt::{Speaker, SttEngine};

/// Layer 1 on its own: the detector must find no speech to hand an engine.
///
/// This runs with no engine at all, so it is the fastest possible signal that
/// the gate is intact, and it is the layer the other two depend on.
#[test]
fn vad_finds_no_speech_in_either_silence_fixture() {
    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let path = fixtures::path(name);
        let pcm = stt::read_wav_16k_mono(&path)
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        let mut vad = EarshotVad::new();
        let spans = detect_speech(&pcm, &mut vad, &SegmentConfig::default());

        assert!(
            spans.is_empty(),
            "{name}: VAD opened {} span(s) on silence, so an engine would be \
             asked to transcribe nothing and could invent text: {spans:?}",
            spans.len()
        );
    }
}

/// Sanity check in the other direction.
///
/// A VAD that returns nothing for *everything* would pass the test above while
/// making the product useless, so the same detector must find speech in the
/// speech fixture. Without this, the silence gate is trivially satisfiable.
#[test]
fn the_same_vad_does_find_speech_in_the_speech_fixture() {
    let pcm = stt::read_wav_16k_mono(&fixtures::path("two-speaker-60s/mic.wav")).unwrap();
    let mut vad = EarshotVad::new();
    let spans = detect_speech(&pcm, &mut vad, &SegmentConfig::default());

    assert!(
        !spans.is_empty(),
        "the VAD found no speech in the speech fixture, which would make the \
         silence test meaningless"
    );
    // Four utterances were spoken. Allow the detector to split or merge a
    // little, but not to collapse the whole track into one span or shred it.
    assert!(
        (2..=8).contains(&spans.len()),
        "expected roughly 4 utterances, got {}: {spans:?}",
        spans.len()
    );
}

#[test]
fn apple_engine_writes_nothing_for_silence() {
    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };

    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!(
            "SKIPPED: Apple's engine is not usable offline here (available={}, installed={})",
            probe.available, probe.installed
        );
        return;
    }

    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let mut engine = stt::apple::AppleEngine::new(binary.clone(), "en-US");
        let mut sink = CollectingSink::new();
        engine
            .transcribe(&fixtures::path(name), Speaker::You, &mut sink)
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        assert!(
            sink.utterances.is_empty(),
            "{name}: Apple's engine invented {} line(s): {:?}",
            sink.utterances.len(),
            sink.lines()
        );
    }
}

#[cfg(all(target_os = "macos", feature = "whisper-model-tests"))]
#[test]
fn whisper_writes_nothing_for_silence() {
    use stt::whisper::{WhisperConfig, WhisperEngine};

    let Some(model) = fixtures::whisper_model() else {
        panic!(
            "whisper-model-tests is on but no model is present. Download one \
             first, or set MEET_WHISPER_MODEL."
        );
    };

    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let mut engine = WhisperEngine::load(&model, WhisperConfig::default()).unwrap();
        let mut sink = CollectingSink::new();
        engine
            .transcribe(&fixtures::path(name), Speaker::You, &mut sink)
            .unwrap_or_else(|e| panic!("{name}: {e}"));

        assert!(
            sink.utterances.is_empty(),
            "{name}: whisper hallucinated {} line(s) over silence — this is the \
             exact Phase 1 failure the VAD gate exists to prevent: {:?}",
            sink.utterances.len(),
            sink.lines()
        );
    }
}

/// The gate as the user experiences it: a silent meeting on disk.
///
/// `transcript.md` must exist and be empty, rather than not existing (the UI
/// has to be able to open it) or containing an invented line.
#[test]
fn a_silent_meeting_produces_an_empty_transcript_file() {
    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };
    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!("SKIPPED: Apple's engine is not usable offline here");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("audio")).unwrap();
    std::fs::copy(
        fixtures::path("silence-30s.wav"),
        root.join("audio/mic.wav"),
    )
    .unwrap();
    std::fs::copy(
        fixtures::path("room-tone-30s.wav"),
        root.join("audio/system.wav"),
    )
    .unwrap();

    let paths = stt::MeetingPaths::new(&root);
    let mut engine = stt::apple::AppleEngine::new(binary, "en-US");
    let outcome = stt::transcribe_meeting(&paths, &mut engine).unwrap();

    assert_eq!(
        outcome.lines, 0,
        "a silent meeting produced transcript lines"
    );
    let body = std::fs::read_to_string(&outcome.transcript_path).unwrap();
    assert_eq!(body, "", "transcript.md should be empty, found: {body:?}");
}
