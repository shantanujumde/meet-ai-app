//! The Parakeet engine (TUR-62) against the same fixtures as whisper and
//! Apple's engine: 30 s of silence and of room tone must write nothing, and
//! the two-speaker meeting must read accurately with each track on its own
//! speaker.
//!
//! Every test here needs the 670 MB Parakeet model, so each is ignored by
//! default and never runs in CI. Run them by hand, once the model is
//! downloaded (Settings, or `MEET_PARAKEET_MODEL=<folder>`):
//!
//! ```sh
//! cargo test -p stt --test parakeet -- --ignored --nocapture
//! ```
//!
//! The fixture WAVs are committed under `crates/audio/fixtures`.

mod fixtures;

use std::path::PathBuf;
use std::time::Instant;

use stt::parakeet::{ParakeetConfig, ParakeetEngine};
use stt::sink::CollectingSink;
use stt::{NoListener, SessionOptions, Speaker, SttEngine};

/// The downloaded model folder: `MEET_PARAKEET_MODEL`, else the default
/// models folder. Never downloads.
fn model() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEET_PARAKEET_MODEL").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let models = stt::model::default_model_dir().expect("a models folder");
    let parakeet = &stt::model::parakeet::PARAKEET_V3;
    assert!(
        parakeet.is_installed(&models),
        "the Parakeet model is not in {}; download it from Settings or set MEET_PARAKEET_MODEL",
        models.display()
    );
    parakeet.dir(&models)
}

fn engine() -> ParakeetEngine {
    ParakeetEngine::load(&model(), ParakeetConfig::default()).expect("load Parakeet")
}

#[test]
#[ignore = "needs parakeet model"]
fn parakeet_writes_nothing_for_silence() {
    let mut engine = engine();
    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let mut sink = CollectingSink::new();
        engine
            .transcribe(&fixtures::path(name), Speaker::You, &mut sink)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            sink.utterances.is_empty(),
            "{name}: Parakeet wrote {} line(s) over silence: {:?}",
            sink.utterances.len(),
            sink.lines()
        );
    }
}

#[test]
#[ignore = "needs parakeet model"]
fn parakeet_live_writes_nothing_for_silence() {
    let mut engine = engine();
    for name in ["silence-30s.wav", "room-tone-30s.wav"] {
        let pcm = stt::read_wav_16k_mono(&fixtures::path(name)).unwrap();
        let sink = stt::SharedSink::new(CollectingSink::new());
        let mut session = engine
            .start_session(
                SessionOptions::new(Speaker::Others),
                Box::new(sink.clone()),
                Box::new(NoListener),
            )
            .unwrap();
        // 100 ms blocks, the size a live tap hands over.
        for block in pcm.chunks(1_600) {
            session.feed(block).unwrap();
        }
        let outcome = session.finish().unwrap();
        assert_eq!(outcome.finalized, 0, "{name}: live Parakeet wrote a line");
    }
}

#[test]
#[ignore = "needs parakeet model"]
fn parakeet_reads_both_tracks_accurately_and_labels_them_correctly() {
    let reference = fixtures::Reference::load();
    let mut engine = engine();

    for (track, speaker) in [
        ("two-speaker-60s/mic.wav", Speaker::You),
        ("two-speaker-60s/system.wav", Speaker::Others),
    ] {
        let mut sink = CollectingSink::new();
        let started = Instant::now();
        engine
            .transcribe(&fixtures::path(track), speaker, &mut sink)
            .unwrap_or_else(|e| panic!("{track}: {e}"));
        let took = started.elapsed().as_secs_f64();

        assert!(!sink.utterances.is_empty(), "{track}: no lines at all");
        for utterance in &sink.utterances {
            assert_eq!(utterance.speaker, speaker, "{track}: {utterance:?}");
        }

        let expected = reference.words_for(speaker);
        let actual: Vec<String> = sink
            .utterances
            .iter()
            .flat_map(|utterance| fixtures::normalize_words(&utterance.text))
            .collect();
        let wer = fixtures::word_error_rate(&expected, &actual);
        // 60 s of audio per track: the real-time factor is the number the
        // CPU-only laptop check in docs/manual-checks/worktree-tur62.md asks for.
        eprintln!(
            "parakeet {track}: WER {:.1}%, {took:.1} s for 60 s (RTF {:.3})",
            wer * 100.0,
            took / 60.0
        );
        eprintln!("  expected: {}", expected.join(" "));
        eprintln!("  actual:   {}", actual.join(" "));
        // The same loose regression alarm as Apple's engine in accuracy.rs.
        assert!(wer < 0.25, "{track}: word error rate {:.1}%", wer * 100.0);
    }
}
