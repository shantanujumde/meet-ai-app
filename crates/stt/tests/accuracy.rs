//! Accuracy and speaker-split, measured rather than eyeballed.
//!
//! SPEC §5 Phase 1 exit gate: "the same recording reads accurately on both
//! engines" and "speakers correctly split". Both are turned into numbers here
//! against `two-speaker-60s/`, whose `reference.json` carries the exact text
//! that was synthesized.
//!
//! ⚠️ **What this does and does not prove.** These fixtures are text-to-speech:
//! no room tone, no codec artefacts, no crosstalk, no accents, no two people
//! talking over each other. They prove the plumbing, the speaker split and the
//! timestamp maths. They do **not** prove real-world accuracy, and the Phase 1
//! gate is explicitly not satisfied by them — that needs a real recording from
//! `meet-rec` ([TUR-4]). The WER thresholds below are therefore loose enough
//! to be a regression alarm, not a quality bar.

mod fixtures;

use stt::sink::CollectingSink;
use stt::{Speaker, SttEngine};

/// Every utterance the mic track should contain, and nothing from the other.
fn assert_speaker_split(utterances: &[stt::Utterance], expected: Speaker) {
    assert!(
        !utterances.is_empty(),
        "no utterances at all, so the split cannot be checked"
    );
    for utterance in utterances {
        assert_eq!(
            utterance.speaker, expected,
            "an utterance from the wrong track leaked in: {utterance:?}"
        );
    }
}

#[test]
fn apple_reads_both_tracks_accurately_and_labels_them_correctly() {
    fixtures::ensure();

    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };
    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!("SKIPPED: Apple's engine is not usable offline here");
        return;
    }

    let reference = fixtures::Reference::load();

    for (track, speaker) in [
        ("two-speaker-60s/mic.wav", Speaker::You),
        ("two-speaker-60s/system.wav", Speaker::Others),
    ] {
        let mut engine = stt::apple::AppleEngine::new(binary.clone(), "en-US");
        let mut sink = CollectingSink::new();
        engine
            .transcribe(&fixtures::path(track), speaker, &mut sink)
            .unwrap_or_else(|e| panic!("{track}: {e}"));

        assert_speaker_split(&sink.utterances, speaker);

        let expected = reference.words_for(speaker);
        let actual: Vec<String> = sink
            .utterances
            .iter()
            .flat_map(|utterance| fixtures::normalize_words(&utterance.text))
            .collect();
        let wer = fixtures::word_error_rate(&expected, &actual);

        eprintln!("apple {track}: WER {:.1}%", wer * 100.0);
        eprintln!("  expected: {}", expected.join(" "));
        eprintln!("  actual:   {}", actual.join(" "));

        // 25% on clean synthetic speech is loose on purpose: it catches "the
        // engine stopped working" without failing the build over a proper noun
        // or a spelled-out numeral. Real accuracy is the TUR-4 gate's job.
        assert!(
            wer < 0.25,
            "{track}: word error rate {:.1}% is too high\n  expected: {}\n  actual:   {}",
            wer * 100.0,
            expected.join(" "),
            actual.join(" ")
        );
    }
}

/// Timestamps must land on the utterance, not near it.
///
/// This is the check that would catch a regression in the clock maths — an
/// off-by-one segment, or a switch back to wall-clock time, would show up here
/// long before anyone noticed it in a real meeting.
#[test]
fn apple_timestamps_land_within_a_second_of_the_real_utterance() {
    fixtures::ensure();

    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };
    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!("SKIPPED: Apple's engine is not usable offline here");
        return;
    }

    let reference = fixtures::Reference::load();
    let mut engine = stt::apple::AppleEngine::new(binary, "en-US");
    let mut sink = CollectingSink::new();
    engine
        .transcribe(
            &fixtures::path("two-speaker-60s/mic.wav"),
            Speaker::You,
            &mut sink,
        )
        .unwrap();

    let expected_starts: Vec<u64> = reference
        .utterances
        .iter()
        .filter(|utterance| utterance.speaker == Speaker::You)
        .map(|utterance| utterance.start_sec)
        .collect();

    // The engine may split one spoken utterance into several results, so every
    // *reference* start has to be matched by some result, not the other way
    // round.
    for expected in expected_starts {
        let closest = sink
            .utterances
            .iter()
            .map(|utterance| utterance.start_sec.abs_diff(expected))
            .min()
            .expect("at least one utterance");
        assert!(
            closest <= 1,
            "no utterance within 1s of the reference start {expected}s \
             (closest was {closest}s away); got {:?}",
            sink.lines()
        );
    }
}

#[test]
fn a_full_meeting_folder_becomes_one_interleaved_transcript() {
    fixtures::ensure();

    let Some(binary) = fixtures::sidecar() else {
        eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
        return;
    };
    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!("SKIPPED: Apple's engine is not usable offline here");
        return;
    }

    let root = std::env::temp_dir().join(format!("meet-ai-meeting-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("audio")).unwrap();
    for name in ["mic.wav", "system.wav", "segments.json"] {
        std::fs::copy(
            fixtures::path(&format!("two-speaker-60s/{name}")),
            root.join("audio").join(name),
        )
        .unwrap();
    }

    let paths = stt::MeetingPaths::new(&root);
    let mut engine = stt::apple::AppleEngine::new(binary, "en-US");
    let outcome = stt::transcribe_meeting(&paths, &mut engine).unwrap();

    let body = std::fs::read_to_string(&outcome.transcript_path).unwrap();
    eprintln!("--- transcript.md ---\n{body}---");

    assert!(outcome.lines >= 6, "only {} lines: {body}", outcome.lines);
    assert!(body.contains("You:"), "no mic-track lines: {body}");
    assert!(body.contains("Others:"), "no system-track lines: {body}");

    // Every line must satisfy the SPEC §3.4 regex and be in timestamp order.
    let mut previous = 0u64;
    for line in body.lines() {
        let (timestamp, rest) = line
            .strip_prefix('[')
            .and_then(|line| line.split_once("] "))
            .unwrap_or_else(|| panic!("line does not match the §3.4 shape: {line:?}"));

        let parts: Vec<&str> = timestamp.split(':').collect();
        assert_eq!(parts.len(), 3, "bad timestamp in {line:?}");
        for part in &parts {
            assert_eq!(part.len(), 2, "timestamp is not zero-padded: {line:?}");
        }
        let seconds = parts[0].parse::<u64>().unwrap() * 3600
            + parts[1].parse::<u64>().unwrap() * 60
            + parts[2].parse::<u64>().unwrap();
        assert!(
            seconds >= previous,
            "transcript is out of order at {line:?}"
        );
        previous = seconds;

        let (speaker, text) = rest.split_once(": ").expect("speaker label");
        assert!(
            speaker == "You" || speaker == "Others",
            "unexpected speaker {speaker:?} in {line:?}"
        );
        assert!(
            !text.trim().is_empty(),
            "empty text reached the file: {line:?}"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}

#[cfg(all(target_os = "macos", feature = "whisper-model-tests"))]
#[test]
fn whisper_reads_the_same_recording_accurately() {
    use stt::whisper::{WhisperConfig, WhisperEngine};

    fixtures::ensure();

    let Some(model) = fixtures::whisper_model() else {
        panic!("whisper-model-tests is on but no model is present");
    };
    eprintln!("whisper model: {}", model.display());

    let reference = fixtures::Reference::load();

    for (track, speaker) in [
        ("two-speaker-60s/mic.wav", Speaker::You),
        ("two-speaker-60s/system.wav", Speaker::Others),
    ] {
        let mut engine = WhisperEngine::load(&model, WhisperConfig::default()).unwrap();
        let mut sink = CollectingSink::new();
        engine
            .transcribe(&fixtures::path(track), speaker, &mut sink)
            .unwrap_or_else(|e| panic!("{track}: {e}"));

        assert_speaker_split(&sink.utterances, speaker);

        let expected = reference.words_for(speaker);
        let actual: Vec<String> = sink
            .utterances
            .iter()
            .flat_map(|utterance| fixtures::normalize_words(&utterance.text))
            .collect();
        let wer = fixtures::word_error_rate(&expected, &actual);

        eprintln!("whisper {track}: WER {:.1}%", wer * 100.0);
        eprintln!("  expected: {}", expected.join(" "));
        eprintln!("  actual:   {}", actual.join(" "));

        // Measured on the pinned small.en-q5_1 model (2026-09-28, TUR-66):
        // mic.wav 3.2%, system.wav 0.0%. Greedy decoding at temperature 0 is
        // deterministic here, so this is not a "happened to land here" number
        // — it reproduced exactly across repeated runs. 15% leaves headroom
        // for a proper noun or a spelled-out numeral while still tripping on
        // an actual regression; the old 30% was loose enough to hide one.
        assert!(
            wer < 0.15,
            "{track}: word error rate {:.1}% is too high\n  expected: {}\n  actual:   {}",
            wer * 100.0,
            expected.join(" "),
            actual.join(" ")
        );
    }
}
