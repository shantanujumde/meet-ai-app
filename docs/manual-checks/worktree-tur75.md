# Manual checks: TUR-75 (pick the speech engine and Whisper model in Settings) and TUR-79 (say what each model is good for)

These need the running, signed app, a microphone and a real call, so none was
run here. What is covered headless:

- `src/ui/engine/EngineSummary.test.tsx`: picking each engine sends the right
  `set_transcription`, Apple and Whisper disabled with the backend's reason,
  picking Whisper also picks a downloaded model, a refused save puts the radios
  back, picking a model, "In use", the "Used only when the engine is Whisper."
  note, the model rows' name / good-for line / tag chips / size and id, the
  "Recommended" mark, and the speech helper path folded under Details.
- `src-tauri/src/config/transcription_tests.rs`: writing `transcription.engine`
  and `transcription.model` changes those two values and nothing else in a
  commented `config.jsonc` (byte-for-byte after swapping them back).
- `src-tauri/src/engine/choices.rs`: the save refuses Apple when it cannot run,
  Whisper when the picked model is not downloaded, and unknown model ids.
- `crates/stt/src/registry/options.rs`: what Automatic lands on, and each
  disabled reason, from one probe and the same decision `resolve` uses.
- `crates/stt/src/model.rs`: every catalogue model has a display name, a
  good-for line and tags with no `q5`/`ggml`/`RTF` in them; the recommended
  rule (large turbo on Apple silicon with 16 GB or more, small otherwise).

## 1. Switch to Whisper + small, record, switch back (TUR-75 "Done when")

1. Run the signed app (`just bundle-signed` on a dev machine) and open
   Settings → Speech.
2. Expected: three radios, "Automatic (recommended)" picked, its line says
   "Uses Apple's engine on this Mac". Under the picker: "Changes apply from the
   next recording. A recording already running keeps its engine."
3. If "Small (English only)" is not on this Mac, press Download on its row and
   wait for it. Expected: when it finishes, the Whisper radio is no longer
   greyed and "Download a model first." is gone.
4. Pick "Whisper". Expected: Whisper is checked; the Small row is checked and
   shows "In use" (if Large turbo is not downloaded, Small is picked for you).
   The "Used only when the engine is Whisper." note disappears.
5. Open `~/Meetings/.app/config.jsonc`. Expected: `"engine": "whisper"`,
   `"model": "small.en-q5_1"` in `transcription`; every comment and other key
   is still there.
6. Record about 1 minute of speech and stop.
7. Expected: the transcript is written, and `meet-ai.log` has an
   `opening live transcription` line with `engine="whisper"` (the log quotes
   the value).
8. Pick "Automatic (recommended)" again. Expected: config says
   `"engine": "auto"`, the models list shows the "Used only when the engine is
   Whisper." note again, and no row says "In use". The next recording's log
   line says `engine="apple-speech"`.

## 2. A recording is never switched mid-way

1. Start a recording with Automatic picked.
2. While it runs, open Settings → Speech and pick Whisper.
3. Expected: the running recording keeps transcribing with Apple's engine
   (log still shows `engine="apple-speech"` for it); the next recording opens
   whisper.

## 3. Disabled states on a real Mac

1. On this Mac (macOS 26, Apple engine ready) Apple is pickable. Expected: its
   line reads "Built into macOS 26. Fast, private, no download. Languages: …"
   with the installed dictation languages in words (e.g. "English (United
   States)").
2. With no Whisper model downloaded (move `~/Meetings/.app/models` aside
   first), Whisper is greyed with "Download a model first." under it.
3. A Mac below macOS 26 cannot be tested (the app needs 26+); the reason text
   for it, "Needs macOS 26 or later.", is covered by
   `options.rs`'s `an_old_mac_says_it_needs_macos_26`.

## 4. Model rows (TUR-79)

1. Expected for each row: a plain name ("Small (English only)", "Large turbo
   (multilingual)"), the good-for sentence, small tag chips (Fast · Light ·
   English only; Most accurate · Multilingual · Slower), then the muted line
   "190 MB · small.en-q5_1 · downloaded once, then kept".
2. Expected: on an Apple silicon Mac with 16 GB or more, the Large turbo row
   says "Recommended · This Mac has Apple silicon and N GB of memory, …"; on
   8 GB, the Small row is the recommended one.
3. Check light and dark mode, and Increase Contrast: the chips and the reason
   lines stay readable; a disabled engine's reason is not dimmed with its
   radio.

## 5. Details toggle and onboarding

1. Expected: no "Speech helper" row is visible until "Details" under the
   models is clicked; then it shows the `meet-stt` path and the locale.
2. Start onboarding again (Settings → Files → Show setup again) and go to the
   speech step. Expected: the same card, with the picker.

## Notes

- The ticket says "Download / Delete stay on the row". There is no Delete
  today (no command removes a model), so the row keeps Download only; adding
  Delete is left for its own ticket.
- `meet-stt --probe` already printed `installed_locales`; `stt::apple::Probe`
  now reads it, so the Apple row lists real languages. No sidecar change.
