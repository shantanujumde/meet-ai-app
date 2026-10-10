# Manual checks: tur156

TUR-156: the whisper text filter (`crates/stt/src/whisper_text.rs`) dropped
real speech and let repeated hallucinations through. The fix:

- A line is a prompt echo only when it is a run of the prompt's words that
  covers more than 60% of them, or that crosses from one prompt sentence into
  the next. "Main doc share kar deta hoon.", "Tum ek baar check kar lena" and
  "next week tak ship ho jayega" are kept. The Hinglish prompt is unchanged.
- whisper's segment is collapsed first (`collapse_repeats`), then checked
  (`whisper_line`), so "Thank you. Thank you. Thank you.", "Bye bye bye bye"
  and "you you you you" are dropped.
- "Okay.", "So.", "Bye.", "Oh.", "Hmm." alone are dropped from whisper only
  (`is_whisper_hallucination`). Parakeet keeps them (`is_hallucination`).
  The no-speech probability was not added to this rule: whisper already drops
  segments above `no_speech_max` before it.
- U+FFFD: when the last word of a segment holds one, that word is dropped
  (whisper stopped mid-word, as designed before). A U+FFFD anywhere else is
  removed and the word is kept. Reading of the ticket's "strip U+FFFD only
  from the final word": the final word itself is stripped.

Ran headless here: `cargo test -p stt` (all green), `cargo clippy -p stt
--all-targets -- -D warnings`, `cargo fmt --all --check`, the quality gate.
No model was loaded and no audio was transcribed.

## Run by hand

Use a build of this branch with whisper as the engine and
`transcription.language` set to `hinglish`.

1. **Short prompt phrases survive.** In a Hindi and English call, say
   "Main doc share kar deta hoon" and "next week tak ship ho jayega".
   Expected: both lines are in `transcript.md`. Skipped here: needs a model
   file, a mic and a real call.
2. **Repeated hallucinations are gone.** Record 30 s of a quiet room with
   whisper. Expected: no "Thank you." (single or repeated) line in
   `transcript.md`. Skipped here: needs a model file and audio.
3. **Parakeet keeps one-word replies.** With `"engine": "parakeet"`, answer
   "Okay." alone after a pause. Expected: an "Okay." line in `transcript.md`.
   Skipped here: needs the Parakeet model and a mic.
4. **Devanagari across segments.** On a Hindi call with whisper, check that
   no word is missing mid-line where whisper split a character (verify it
   against the audio). Skipped here: needs a model file and real speech.
