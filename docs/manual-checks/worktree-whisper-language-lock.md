# Manual checks: whisper-language-lock

Owner's spoken-language work (SPEC A22, `transcription.language`): auto keeps
each line's language with a per-speaker fallback, a "Spoken language" picker in
Settings → Speech, Hinglish via a prompt, and loop cutting. The pure logic
(`spoken_language.rs`, `whisper_text.rs`, config parsing, the picker) is
unit-tested headless. Nothing below was run here: each needs a whisper model
file, a real meeting or the signed app.

## Run by hand

1. Settings → Speech with a multilingual whisper model (large-v3-turbo).
   Expect: a "Spoken language" select with Automatic, Hinglish, then languages
   by name; picking `Marathi` writes `"language": "mr"` to config.jsonc.
   Why skipped: needs the running app.
2. Record a Hindi and English call with Automatic.
   Expect: Hindi lines in Hindi, English lines in English; short lines ("Hmm")
   follow the speaker's usual language, not stray Chinese/Tamil/etc.
   Why skipped: needs a model file and a real meeting.
3. Same call with Hinglish.
   Expect: Hindi written in English letters as spoken, English lines
   unchanged, no line that is only the prompt sentences.
   Why skipped: needs a model file and a real meeting.
4. A Marathi call with `mr` on large-v3-turbo and large-v3.
   Expect: Devanagari Marathi; no line that loops one word or phrase.
   Why skipped: needs a model file and a real meeting.
5. With `small.en-q5_1`, and with Apple or Parakeet, set any language.
   Expect: unchanged English output; the setting is ignored.
   Why skipped: needs a model file / the running app.
6. Hand-edit `"language": "xx"` in config.jsonc and record.
   Expect: treated as auto, a log line about the unknown value, engine and
   model still read.
   Why skipped: needs the running app.

## Known

- SPEC had two "A17" amendments; this one is now A22 (D25).
