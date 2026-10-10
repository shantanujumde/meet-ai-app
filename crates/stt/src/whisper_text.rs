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
];

/// One-word replies people really say on a call, which whisper also invents
/// over quiet audio. Dropped from whisper's output only
/// ([`is_whisper_hallucination`]): Parakeet does not make these up, so there a
/// standalone "Okay." is a real answer and is kept (TUR-156).
const WHISPER_BARE_WORDS: &[&str] = &["so", "okay", "bye", "oh", "hmm"];

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

/// [`is_hallucination`], plus the bare one-word replies in
/// [`WHISPER_BARE_WORDS`] that only whisper makes up.
pub fn is_whisper_hallucination(text: &str) -> bool {
    is_hallucination(text) || WHISPER_BARE_WORDS.contains(&bare(text.trim()).as_str())
}

/// One raw whisper segment as the line it becomes, or `None` when it is
/// dropped. The order matters: a loop is cut to one copy first, so "Thank
/// you. Thank you. Thank you." is judged as the "Thank you." it is, and only
/// then are the hallucination and prompt-echo rules run on what is left.
pub fn whisper_line(raw: &str, prompt: Option<&str>) -> Option<String> {
    let text = crate::collapse_whitespace(&collapse_repeats(raw))?;
    if is_whisper_hallucination(&text) {
        tracing::debug!(text = %raw, "dropped: known hallucination phrase");
        return None;
    }
    if prompt.is_some_and(|prompt| echoes_prompt(&text, prompt)) {
        tracing::debug!(text = %text, "dropped: the prompt written back");
        return None;
    }
    Some(text)
}

/// The longest phrase, in words, looked for as a loop.
const MAX_PHRASE_WORDS: usize = 10;

/// Copies of one word in a row that make a loop. Higher than for a phrase,
/// so "no, no, no" stays as it was said.
const WORD_REPEATS: usize = 4;

/// Copies of a phrase of two words or more in a row that make a loop.
const PHRASE_REPEATS: usize = 3;

/// A line is a prompt echo only when it covers more than this share of the
/// prompt's words, in percent (or spans two of its sentences). The prompt is
/// ordinary meeting talk, so people say its shorter phrases for real:
/// "Main doc share kar deta hoon." is speech, not an echo (TUR-156).
const MIN_ECHO_SHARE_PERCENT: usize = 60;

/// `text` with every run of [`WORD_REPEATS`] words, or [`PHRASE_REPEATS`]
/// phrases, cut to one copy. A run that ends in a cut-off copy of the phrase
/// (whisper running out of tokens mid-word) loses that piece too.
///
/// U+FFFD is half a character. On the last word it means whisper stopped
/// mid-word, so that word goes. Anywhere else it is a character split across
/// tokens; the word is real and is kept with the U+FFFD taken out.
pub fn collapse_repeats(text: &str) -> String {
    let mut owned: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    if owned.last().is_some_and(|word| word.contains('\u{FFFD}')) {
        owned.pop();
    }
    let words: Vec<&str> = owned
        .iter_mut()
        .map(|word| {
            word.retain(|c| c != '\u{FFFD}');
            word.as_str()
        })
        .filter(|word| !word.is_empty())
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

/// Is `text` the prompt written back? Only when it is a run of the prompt's
/// words that covers more than [`MIN_ECHO_SHARE_PERCENT`] of them, or that
/// crosses from one prompt sentence into the next. A shorter piece inside one
/// sentence is something people say.
pub fn echoes_prompt(text: &str, prompt: &str) -> bool {
    let said: Vec<String> = text
        .split_whitespace()
        .map(bare)
        .filter(|word| !word.is_empty())
        .collect();
    // Each prompt word with the number of the sentence it is in.
    let mut sentence = 0;
    let mut words: Vec<(String, usize)> = Vec::new();
    for raw in prompt.split_whitespace() {
        let word = bare(raw);
        if !word.is_empty() {
            words.push((word, sentence));
        }
        if raw.ends_with(['.', '?', '!', '।']) {
            sentence += 1;
        }
    }
    if said.is_empty() || said.len() > words.len() {
        return false;
    }
    let large = said.len() * 100 > words.len() * MIN_ECHO_SHARE_PERCENT;
    words.windows(said.len()).any(|run| {
        let matches = run.iter().zip(&said).all(|((word, _), s)| word == s);
        let first = run.first().map(|(_, n)| *n);
        let last = run.last().map(|(_, n)| *n);
        matches && (large || first != last)
    })
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

    const PROMPT: &str = crate::languages::HINGLISH_PROMPT;

    #[test]
    fn a_line_that_is_most_of_the_prompt_is_an_echo() {
        assert!(echoes_prompt(PROMPT, PROMPT));
        assert!(echoes_prompt(
            "Mujhe lagta hai ye feature next week tak ship ho jayega. Main doc share kar deta hoon, tum ek baar check kar lena.",
            PROMPT
        ));
        assert_eq!(whisper_line(PROMPT, Some(PROMPT)), None);
    }

    #[test]
    fn a_line_that_crosses_two_prompt_sentences_is_an_echo() {
        assert!(echoes_prompt("kya decide hua tha? Mujhe lagta hai", PROMPT));
    }

    #[test]
    fn a_hinglish_line_that_is_a_short_piece_of_the_prompt_is_kept() {
        for line in [
            "Main doc share kar deta hoon.",
            "Tum ek baar check kar lena",
            "next week tak ship ho jayega",
            "Kal ki meeting mein kya decide hua tha?",
            "Haan bhai.",
        ] {
            assert!(!echoes_prompt(line, PROMPT), "{line:?} should be kept");
            assert_eq!(whisper_line(line, Some(PROMPT)).as_deref(), Some(line));
        }
        // Shares words with the prompt but is its own sentence.
        assert!(!echoes_prompt("Kal ki meeting cancel ho gayi thi.", PROMPT));
    }

    #[test]
    fn a_repeated_hallucination_is_dropped_after_it_is_collapsed() {
        for line in [
            "Thank you. Thank you. Thank you.",
            "Bye bye bye bye",
            "you you you you",
        ] {
            assert!(!is_hallucination(line), "{line:?} is only caught collapsed");
            assert_eq!(whisper_line(line, None), None, "{line:?} should be dropped");
        }
    }

    #[test]
    fn bare_one_word_replies_are_whisper_only_hallucinations() {
        for word in ["Okay.", "So.", "Bye.", "Oh.", "Hmm.", " okay "] {
            assert!(is_whisper_hallucination(word), "{word:?}");
            assert!(!is_hallucination(word), "{word:?} is real on Parakeet");
        }
        assert_eq!(
            whisper_line("Okay, let's ship it.", None).as_deref(),
            Some("Okay, let's ship it.")
        );
    }

    #[test]
    fn a_mid_line_half_character_is_removed_and_the_word_kept() {
        assert_eq!(
            collapse_repeats("हम कल \u{FFFD}मीटिंग में बात करेंगे"),
            "हम कल मीटिंग में बात करेंगे"
        );
        assert_eq!(collapse_repeats("हम कल मीटिं\u{FFFD} में"), "हम कल मीटिं में");
        // On the last word it means whisper stopped mid-word: the stub goes.
        assert_eq!(collapse_repeats("हम कल मीटिं\u{FFFD}"), "हम कल");
        assert_eq!(collapse_repeats("\u{FFFD} हम"), "हम");
    }
}
