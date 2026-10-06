//! Repairs for two ways whisper's text goes wrong that the hallucination guard
//! in `whisper.rs` does not cover.
//!
//! * **Loops.** Unsure of a span, greedy decoding with no temperature fallback
//!   can repeat one word or phrase until it runs out of tokens: "लिए लिए लिए
//!   …", "aaa, aaa, aaa, …", "बोलना पाईजी तो बोलना पाईजी तो …". Measured on a
//!   real Marathi call and a Hindi and English one, it happened on roughly one
//!   line in ten when whisper was told Hindi or Marathi. [`collapse_repeats`]
//!   keeps one copy.
//! * **Prompt echoes.** With a prompt (Hinglish's example sentences), whisper
//!   sometimes writes the prompt back out over a short or unclear span.
//!   [`echoes_prompt`] spots a line that is only a piece of the prompt.
//!
//! Also home to layer 3 of the hallucination guard ([`is_hallucination`],
//! re-exported as `stt::whisper::is_hallucination`), and where the prompt
//! becomes tokens ([`prompt_tokens`]).

use whisper_rs::{WhisperContext, WhisperTokenId};

use crate::Error;

/// whisper keeps at most half its 448-token text context for a prompt.
const MAX_PROMPT_TOKENS: usize = 224;

/// `prompt` as whisper tokens, made once per engine. Passed to every decode as
/// tokens rather than text because whisper-rs leaks the C string it makes for
/// a text prompt, once per call.
pub fn prompt_tokens(
    context: &WhisperContext,
    prompt: Option<&str>,
) -> Result<Vec<WhisperTokenId>, Error> {
    match prompt {
        None => Ok(Vec::new()),
        Some(prompt) => context
            .tokenize(prompt, MAX_PROMPT_TOKENS)
            .map_err(|e| Error::Engine(format!("could not read the whisper prompt: {e}"))),
    }
}

/// Phrases whisper invents over quiet audio.
///
/// Compared case-insensitively against the whole trimmed segment, with
/// punctuation stripped. A segment equal to one of these and nothing else is
/// dropped. Sourced from the widely reported whisper.cpp hallucination set —
/// these are subtitle-corpus artefacts, not English that shows up alone in a
/// meeting.
const HALLUCINATION_PHRASES: &[&str] = &[
    "thank you",
    "thanks for watching",
    "thank you for watching",
    "thanks for watching!",
    "you",
    "bye",
    "bye bye",
    "thank you very much",
    "please subscribe",
    "subscribe to my channel",
    "blank_audio",
    "silence",
    "music",
    "applause",
    "inaudible",
    "beep",
    "so",
    "okay",
    "oh",
    "hmm",
];

/// Is this segment nothing but a known hallucination?
///
/// Two rules, in order:
///
/// 1. **Shape.** A segment that is entirely wrapped in `[...]`, `(...)` or
///    `*...*` is a sound annotation, not speech — `[BLANK_AUDIO]`,
///    `[no speech detected]`, `(water rushing)`, `*door closes*`. This rule is
///    the important one because it is structural: it catches annotations
///    nobody has seen yet, which an enumerated list by definition cannot.
///    Measured against this repo's fixtures, ungated whisper emitted
///    `[BLANK_AUDIO]`, `[no speech detected]` and `(water rushing)` over
///    silence and pink noise — only the first was on the list below.
/// 2. **A phrase list**, for bare-text hallucinations that carry no brackets.
///
/// Public so the test suite can assert the rule directly, and so a future
/// Silero swap can reuse it unchanged.
pub fn is_hallucination(text: &str) -> bool {
    let trimmed = text.trim();

    // Rule 1. Checked before punctuation is stripped, because the brackets
    // are the entire signal.
    let wrapped = [('[', ']'), ('(', ')'), ('*', '*'), ('<', '>'), ('{', '}')]
        .iter()
        .any(|(open, close)| {
            trimmed.starts_with(*open)
                && trimmed.ends_with(*close)
                && trimmed.chars().count() >= 2
                // Only if there is exactly one bracketed run, so a real
                // sentence like "(see the ticket) and then we ship" is kept.
                && !trimmed[1..trimmed.len() - close.len_utf8()].contains(*close)
        });
    if wrapped {
        return true;
    }

    let normalized: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.is_empty() {
        return true;
    }
    HALLUCINATION_PHRASES.contains(&normalized.as_str())
}

/// The longest phrase, in words, looked for as a loop.
const MAX_PHRASE_WORDS: usize = 10;

/// Copies of one word in a row that make a loop. Higher than for a phrase,
/// so "no, no, no" stays as it was said.
const WORD_REPEATS: usize = 4;

/// Copies of a phrase of two words or more in a row that make a loop.
const PHRASE_REPEATS: usize = 3;

/// Shorter lines are never treated as a prompt echo: "theek hai" is something
/// people say, not only something the prompt contains.
const MIN_ECHO_CHARS: usize = 16;

/// `text` with every run of [`WORD_REPEATS`] words, or [`PHRASE_REPEATS`]
/// phrases, cut to one copy. A run that ends in a cut-off copy of the phrase
/// (whisper running out of tokens mid-word) loses that piece too, as does a
/// word whisper ended with half a character (U+FFFD).
pub fn collapse_repeats(text: &str) -> String {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !word.contains('\u{FFFD}'))
        .collect();
    let mut kept: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        match longest_loop(&words[i..]) {
            Some((len, copies)) => {
                let phrase = &words[i..i + len];
                kept.extend_from_slice(phrase);
                i += len * copies;
                let tail = &words[i..];
                if tail.len() <= len && is_cut_off_copy(tail, phrase) {
                    i = words.len();
                }
            }
            None => {
                kept.push(words[i]);
                i += 1;
            }
        }
    }
    kept.join(" ")
}

/// The phrase length and number of copies of a loop starting at `words[0]`.
fn longest_loop(words: &[&str]) -> Option<(usize, usize)> {
    (1..=MAX_PHRASE_WORDS.min(words.len() / 2)).find_map(|len| {
        let phrase = &words[..len];
        let copies = words
            .chunks_exact(len)
            .take_while(|chunk| same_words(chunk, phrase))
            .count();
        let needed = if len == 1 {
            WORD_REPEATS
        } else {
            PHRASE_REPEATS
        };
        (copies >= needed).then_some((len, copies))
    })
}

/// Is `tail` the start of `phrase`, its last word possibly cut short?
fn is_cut_off_copy(tail: &[&str], phrase: &[&str]) -> bool {
    !tail.is_empty()
        && tail
            .iter()
            .zip(phrase)
            .enumerate()
            .all(|(n, (word, full))| {
                let (word, full) = (bare(word), bare(full));
                if n + 1 == tail.len() {
                    full.starts_with(&word)
                } else {
                    word == full
                }
            })
}

fn same_words(a: &[&str], b: &[&str]) -> bool {
    a.iter().zip(b).all(|(x, y)| bare(x) == bare(y))
}

/// A word without the punctuation around it, lowercased.
fn bare(word: &str) -> String {
    word.trim_matches(|c: char| c.is_ascii_punctuation() || c == '।' || c == '…')
        .to_lowercase()
}

/// Is `text` nothing but a piece of `prompt`?
pub fn echoes_prompt(text: &str, prompt: &str) -> bool {
    let text = plain(text);
    text.chars().count() >= MIN_ECHO_CHARS && plain(prompt).contains(&text)
}

/// Lowercase words with the punctuation dropped, one space apart.
fn plain(text: &str) -> String {
    text.split_whitespace()
        .map(bare)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_looping_word_is_kept_once() {
        assert_eq!(collapse_repeats("जो जो जो जो जो जो जो जो जो जो"), "जो");
        assert_eq!(collapse_repeats("aaa, aaa, aaa, aaa, aaa, aaa,"), "aaa,");
    }

    #[test]
    fn a_looping_phrase_is_kept_once_with_what_came_before() {
        assert_eq!(
            collapse_repeats(
                "तो तुम्हें असो बोलना पाईजी तोसा बोलना पाईजी तोसा बोलना पाईजी तोसा बोलना पाईजी तो रहले"
            ),
            "तो तुम्हें असो बोलना पाईजी तोसा बोलना पाईजी तो रहले"
        );
        assert_eq!(
            collapse_repeats("AI शोट बेब टो रिजोल रिजोल रिजोल रिजोल रिजोल रिजोल"),
            "AI शोट बेब टो रिजोल"
        );
    }

    #[test]
    fn a_loop_cut_off_mid_word_loses_the_stub() {
        assert_eq!(
            collapse_repeats("आईश्य उर्बगाईची एची एची एची एची एची ए\u{FFFD}"),
            "आईश्य उर्बगाईची एची"
        );
        assert_eq!(
            collapse_repeats("we said it lekhab lekhab lekhab lekhab lek"),
            "we said it lekhab"
        );
    }

    #[test]
    fn ordinary_speech_is_left_alone() {
        for line in [
            "No, no, no, that is not what I meant.",
            "Yes, yes. Follow-up closure.",
            "if AI feels resolution ho gaya hai, toh close kar do",
            "bye bye",
            "",
        ] {
            assert_eq!(
                collapse_repeats(line),
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            );
        }
    }

    #[test]
    fn a_line_that_is_only_part_of_the_prompt_is_an_echo() {
        let prompt = "Haan bhai, kal ki meeting mein kya decide hua tha? Mujhe lagta hai ye feature next week tak ship ho jayega.";
        assert!(echoes_prompt(
            "Kal ki meeting mein kya decide hua tha?",
            prompt
        ));
        assert!(echoes_prompt(prompt, prompt));
        // Short and common: said, not echoed.
        assert!(!echoes_prompt("Haan bhai.", prompt));
        // Shares words with the prompt but is its own sentence.
        assert!(!echoes_prompt("Kal ki meeting cancel ho gayi thi.", prompt));
    }
}
