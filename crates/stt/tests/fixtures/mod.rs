//! Shared fixture plumbing for the Phase 1 test suite.
//!
//! The WAVs live in `crates/audio/fixtures/` because SPEC §2.9 and §6 put them
//! there — one fixture set for the whole workspace, not one per crate. They are
//! generated rather than committed (see `generate.sh`), so every test that
//! needs them calls [`ensure`] first and skips loudly if the generator has not
//! been run.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

/// `crates/audio/fixtures/`.
pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("audio")
        .join("fixtures")
}

pub fn path(relative: &str) -> PathBuf {
    dir().join(relative)
}

static GENERATE: Once = Once::new();

/// Generate the fixture WAVs if they are not already there.
///
/// Running the generator from the test suite rather than requiring
/// `just fixtures` first means a fresh clone can `cargo test -p stt` and have
/// it work. It is a no-op once the files exist.
pub fn ensure() {
    GENERATE.call_once(|| {
        let marker = path("silence-30s.wav");
        if marker.is_file() {
            return;
        }
        let script = dir().join("generate.sh");
        eprintln!("generating fixtures with {}", script.display());
        let status = Command::new("bash").arg(&script).status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("fixture generation exited with {status}"),
            Err(error) => eprintln!("could not run {}: {error}", script.display()),
        }
    });
}

/// The `meet-stt` sidecar, if `just sidecar` has built it.
///
/// Returns `None` rather than failing so the suite still runs usefully on a
/// machine where the Swift side has not been built — the tests that need it
/// say so out loud when they skip.
pub fn sidecar() -> Option<PathBuf> {
    if let Ok(from_env) = std::env::var("MEET_STT_BIN") {
        let path = PathBuf::from(from_env);
        return path.is_file().then_some(path);
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/meet-stt")
        .canonicalize()
        .ok()?;
    candidate.is_file().then_some(candidate)
}

/// A downloaded whisper model, if one is present.
///
/// Never downloads. The whisper tests are opt-in behind the
/// `whisper-model-tests` feature precisely so that `just check` does not need
/// 190 MB and a network connection.
pub fn whisper_model() -> Option<PathBuf> {
    if let Ok(from_env) = std::env::var("MEET_WHISPER_MODEL") {
        let path = PathBuf::from(from_env);
        return path.is_file().then_some(path);
    }
    let dir = stt::model::default_model_dir().ok()?;
    stt::model::MODELS
        .iter()
        .map(|spec| dir.join(spec.filename))
        .find(|path| path.is_file())
}

/// The ground truth for `two-speaker-60s/`.
#[derive(Debug, serde::Deserialize)]
pub struct Reference {
    pub utterances: Vec<ReferenceUtterance>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ReferenceUtterance {
    pub start_sec: u64,
    pub speaker: stt::Speaker,
    pub text: String,
}

impl Reference {
    pub fn load() -> Self {
        let body = std::fs::read_to_string(path("two-speaker-60s/reference.json"))
            .expect("reference.json — run crates/audio/fixtures/generate.sh");
        serde_json::from_str(&body).expect("reference.json is valid JSON")
    }

    /// Every reference word spoken on one track, lowercased and depunctuated.
    pub fn words_for(&self, speaker: stt::Speaker) -> Vec<String> {
        self.utterances
            .iter()
            .filter(|utterance| utterance.speaker == speaker)
            .flat_map(|utterance| normalize_words(&utterance.text))
            .collect()
    }
}

/// Lowercase, strip punctuation, split on whitespace.
///
/// Word error rate is a measure of what was *said*, not of how an engine
/// chose to punctuate it. Comparing raw strings would score Apple's
/// "Sessions are still in memory. That is the blocker." as two errors against
/// one reference line purely for putting a full stop in the middle.
pub fn normalize_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Word error rate: Levenshtein distance over words, divided by the reference
/// length. 0.0 is perfect; 1.0 means nothing matched.
pub fn word_error_rate(reference: &[String], hypothesis: &[String]) -> f64 {
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }

    // Classic edit-distance table, one row at a time.
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut current = vec![0usize; hypothesis.len() + 1];

    for (i, reference_word) in reference.iter().enumerate() {
        current[0] = i + 1;
        for (j, hypothesis_word) in hypothesis.iter().enumerate() {
            let substitution = previous[j] + usize::from(reference_word != hypothesis_word);
            let deletion = previous[j + 1] + 1;
            let insertion = current[j] + 1;
            current[j + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[hypothesis.len()] as f64 / reference.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_transcripts_score_zero() {
        let words = normalize_words("Sessions are still in memory.");
        assert_eq!(word_error_rate(&words, &words), 0.0);
    }

    #[test]
    fn one_wrong_word_in_five_scores_twenty_percent() {
        let reference = normalize_words("moving them into Redis now");
        let hypothesis = normalize_words("moving them into release now");
        assert!((word_error_rate(&reference, &hypothesis) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn punctuation_and_case_are_not_errors() {
        let reference = normalize_words("Sessions are still in memory, that is the blocker.");
        let hypothesis = normalize_words("sessions are still in memory  That is the blocker");
        assert_eq!(word_error_rate(&reference, &hypothesis), 0.0);
    }

    #[test]
    fn an_empty_hypothesis_is_a_total_miss() {
        let reference = normalize_words("anything at all");
        assert_eq!(word_error_rate(&reference, &[]), 1.0);
    }
}
