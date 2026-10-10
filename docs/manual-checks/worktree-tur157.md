# TUR-157 manual checks

## 1. Picker state in the running app, per engine and model

- Run: in a signed build on macOS 26, open Settings, Speech. Try each of: Automatic (lands on Apple's engine), Whisper with `small.en`, Whisper with Large turbo, Parakeet (model downloaded).
- Expected: only Whisper with a multilingual model leaves "Spoken language" enabled with its usual line. The others show it disabled with one line: "This model only transcribes English. ...", "Apple's speech engine transcribes in en-US and does not use this setting." or "Parakeet works out the language on its own ...". The saved language (for example Marathi) stays selected and comes back into effect when you switch to a multilingual whisper model.
- Skipped: needs the running app and downloaded models. Unit tests cover the flag (`crates/stt/src/registry/language.rs`, `src-tauri/src/engine/choices.rs`) and the picker (`src/ui/engine/SpokenLanguagePicker.test.tsx`).

## 2. Layout of the reason line

- Run: same screen, with the picker disabled.
- Expected: the reason wraps in the row's grey line like the other rows; the disabled select looks disabled.
- Skipped: jsdom has no layout.
