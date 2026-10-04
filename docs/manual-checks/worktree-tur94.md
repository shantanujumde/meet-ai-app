# Manual checks: TUR-94

Whisper was told every recording was English, so the multilingual models wrote
Hindi as made-up English. The language now comes from the model
(`ModelSpec::whisper_language`: `en` for `small.en-q5_1`, auto-detect for the
rest), Settings lists Small, Medium, Large turbo and Large, and each model row
has an (i) button with its language list.

The headless tests prove the language each model is loaded with, the model
list, the language lists and the (i) button in jsdom. They cannot prove what
whisper writes for real speech, that the new files download, or how the panel
looks, so these need the signed app. None were run here: no models may be
downloaded in this worktree, and there is no signed build, microphone or call.

Setup for 1 to 4: a signed build (`just bundle-signed`), Settings > Speech,
engine set to Whisper.

## Run by hand

1. **Hindi with Large and with Large turbo.** Download "Large (multilingual)",
   pick it, record a call where the other side speaks only Hindi for a minute.
   Repeat with "Large turbo (multilingual)".
   Expect: the live transcript and `transcript.md` are Hindi in Devanagari
   script, not English words. (Before this fix turbo wrote things like "I'm
   going to go to Hindi.")
   Why skipped: needs a real call, the signed app and a 0.5 to 1 GB download.

2. **Hinglish call.** With Large, record a call that mixes Hindi and English in
   the same sentences.
   Expect: Hindi parts in Devanagari (or romanised Hindi), English parts in
   English; nothing translated. Auto-detect runs per utterance, so a short
   utterance may be put in the wrong language now and then; note how often.
   Why skipped: as 1.

3. **English with Small is unchanged.** Pick "Small (English only)", record an
   English call.
   Expect: the same quality as before this change (Small still runs with `en`).
   Why skipped: as 1.

4. **Download Medium and Large.** Click Download on "Medium (multilingual)"
   (539 MB) and "Large (multilingual)" (1.08 GB).
   Expect: progress reaches 100%, the checksum passes, the row shows "On this
   Mac" and can be picked. A wrong pinned digest would show the checksum error
   instead.
   Why skipped: downloads are not allowed in this worktree.

5. **(i) panel, light and dark.** On each of the four rows click the (i)
   button, in light and then dark appearance, and once with Increase Contrast.
   Expect: Small says "English only"; Medium "99 languages"; Large turbo and
   Large "100 languages" (Cantonese included). The list is A to Z and
   scrolls, the accuracy note and "Source: OpenAI Whisper" are readable, the
   link opens the browser, Escape and a click outside close it and focus goes
   back to the button, and clicking (i) never changes the picked model.
   Why skipped: needs the running app.

## Decisions to note

- The SPEC amendment is A16, placed above A15 to keep the file's
  newest-first order, rather than after A1 at the very bottom.
- Medium's text says it is faster than Large but that Large turbo is faster
  still: OpenAI's README lists medium at ~2x the speed of large and turbo at
  ~8x, and the q5 files are about the same size (539 MB and 574 MB).
- The recommendation reason no longer calls turbo "the most accurate model".
