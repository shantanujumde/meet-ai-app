# tur52 manual checks

Windows and Linux could not be run from this Mac. GitHub Actions builds and
tests the `stt` crate natively there (the `rust (windows)` and `rust (linux)`
jobs in `.github/workflows/check.yml`), but without a whisper model, so CI only
proves that whisper.cpp builds portably, links and loads (typed errors on a
missing or garbage model). Nothing below was run.

1. **The CI-built Windows app transcribes a test recording.**
   Download the `meet-ai-windows-nsis` artifact from a green `check` run on a
   push to main (PR runs skip the Windows bundle), install it on Windows 10/11,
   put `ggml-small.en-q5_1.bin` (or the configured model) in the app's model
   folder or download it from Settings, set `"engine": "auto"`, and record or
   import a short meeting with speech on both tracks.
   Expected: Settings shows whisper as the engine; `transcript.md` gets
   `[HH:MM:SS] You: ...` / `Others: ...` lines; no crash.
   Skipped: needs a real Windows machine, a model download and a person.

2. **No illegal-instruction crash on an older CPU.**
   Run the same installer on an x86-64 machine without AVX-512 (any pre-2017
   Intel or pre-Zen 4 AMD), transcribe as in 1.
   Expected: works. CI checks `GGML_NATIVE:BOOL=OFF` in whisper.cpp's
   CMakeCache.txt (step "whisper.cpp was built portably"), not the CPU itself.
   Note: Meetily's include keeps the x64 baseline with AVX2, so a CPU without
   AVX2 (pre-2013) may still fail; that is accepted for now.
   Skipped: needs old hardware.

3. **Linux transcribes.** As 1 with the `meet-ai-linux-deb` artifact on
   Ubuntu 24.04 desktop. Skipped: needs a Linux desktop session.

4. **Forcing apple-speech off macOS.** On Windows or Linux set
   `"engine": "apple-speech"` in `config.jsonc` and open Settings.
   Expected: "Apple's speech engine cannot be used: it is part of macOS and does
   not exist on this system; set "engine" to "auto" or "whisper"". The unit test
   `forcing_apple_where_it_cannot_exist_ignores_a_sidecar_on_disk` covers the
   message on the Windows/Linux CI runners; the UI wording is not checked.
   Skipped: needs the running app.

5. **Whisper fixture tests on Windows/Linux (for TUR-50).**
   `crates/stt/tests/silence.rs` and `live.rs` are
   `#![cfg(target_os = "macos")]`, and the whisper test in `accuracy.rs`
   (two-speaker) is `cfg(all(target_os = "macos", feature = ...))`, because `crates/audio/fixtures/generate.sh`
   uses macOS `say`, and their whisper halves also need a model
   (`whisper-model-tests` feature, never on in CI). Once TUR-50 makes the
   fixtures portable: `cargo test -p stt --features whisper-model-tests` with
   `MEET_WHISPER_MODEL` set, on Windows and Linux. Expected: zero lines on both
   silence fixtures, the two-speaker accuracy bar met.
   Skipped: fixtures need `say`; the model download is not allowed in CI.

Follow-ups: `whisper-guard` `clean_segments` was not added (whisper.rs already
has a hallucination filter; wiring the crate is its own change). GPU off macOS
is TUR-61.
