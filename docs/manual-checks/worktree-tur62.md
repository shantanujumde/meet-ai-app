# Manual checks: TUR-62 (Parakeet speech engine, ONNX on the CPU)

`"engine": "parakeet"` is a third speech engine: NVIDIA
`parakeet-tdt-0.6b-v3` (int8 ONNX export) through `parakeet-rs` 0.3.8 on
ONNX Runtime, CPU only on every OS. It is VAD-chunked exactly like whisper
(batch: `detect_speech`; live: `SpanAssembler`) and goes through the same
phrase rule (`whisper::is_hallucination`). `Auto` never picks it (TUR-61 owns
`Auto`). Settings, Speech has a Parakeet row with a Download button (670 MB,
three files, each pinned by SHA-256 and fetched through `modelfetch`), and
Settings, About shows the CC-BY-4.0 credit.

## What was run here, headless (macOS, Apple silicon)

- `cargo test -p stt -p modelfetch -p meet-ai`: all pass. New tests:
  - `stt` `parakeet::tests`: audio scaling and padding, sentences to lines on
    the recording timeline, the phrase rule, a missing folder is
    `ModelMissing`, a folder of garbage files is a typed `Engine` error from
    parakeet-rs/ONNX Runtime (proves ORT links and runs, no model needed),
    and the `onnxruntime.dll` path rule (override, then next to the exe).
  - `stt` `model::parakeet::tests`: pinned URLs and digests, the file names
    parakeet-rs looks for, "installed" means every file under its finished
    name (a `.part` does not count), plain words on the row.
  - `stt` `registry` and `registry::parakeet`: `Preference::Parakeet` only
    with the model, the error names the model, a missing ONNX Runtime is
    named first (with the model or without), `Auto` unchanged,
    `"parakeet"` in config, discovery only finds a complete folder; the
    picker says "not ready on this system" before it asks for a download.
  - `stt` `parakeet::tests::a_missing_onnx_runtime_names_the_file_and_where_it_was_looked_for`:
    the Windows message with a missing path, and no message once the file
    is there.
  - `modelfetch` `folder::tests`: one progress bar over several files, an
    installed folder makes no request, a bad file stops with its own checksum
    error.
  - `meet-ai`: `EngineChoice::Parakeet`, refused until downloaded,
    `download_model("parakeet-tdt-0.6b-v3")` maps to the folder, discovery
    passes the folder to the registry (stranded copy too), schema enum lists
    `"parakeet"`, credits.
- `cargo clippy -p stt -p modelfetch -p meet-ai --all-targets -- -D warnings`,
  `cargo fmt --all --check`.
- `pnpm vitest run` (all 50 files), `pnpm typecheck`, `pnpm biome check` on the
  changed files. New: the Parakeet row (disabled with reason, Download calls
  `downloadModel("parakeet-tdt-0.6b-v3")`, pickable once downloaded, a failed
  download shows under the row) and `AboutSettings`.
- `crates/stt/src/platform/onnx_windows.rs` was compiled for
  `x86_64-pc-windows-msvc` in a scratch crate (`ort` 2.0.0-rc.13 with
  `load-dynamic`). `crates/stt` itself cannot be cross-checked from a Mac
  (whisper.cpp and Oniguruma compile C for the target); the `rust (windows)`
  CI job builds and tests it.
- Model sizes and SHA-256 come from the Hugging Face API for
  `istupakov/parakeet-tdt-0.6b-v3-onnx` at commit
  `8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce` (LFS object ids). `vocab.txt`
  (94 KB, a plain git file, so the API has no SHA-256 for it) was fetched
  once to hash it; its git blob id matched the API's. No model file was
  downloaded.

## Needs a person

Each of these needs the 670 MB model, a real machine or a signed build, so
none ran here.

1. **Silence and two-speaker fixtures with the real model** (every OS whose
   fixtures exist; today macOS, `say`-generated, until TUR-50 lands).
   Download the model from Settings (or put the three files in a folder and
   set `MEET_PARAKEET_MODEL=<folder>`), then:
   `cargo test -p stt --test parakeet -- --ignored --nocapture`.
   Expected: `parakeet_writes_nothing_for_silence` and
   `parakeet_live_writes_nothing_for_silence` pass (zero lines over
   `silence-30s.wav` and `room-tone-30s.wav`);
   `parakeet_reads_both_tracks_accurately_and_labels_them_correctly` passes
   with WER under 25% per track and prints `RTF` per 60 s track. Skipped here:
   needs the model download.
2. **CPU-only Windows laptop keeps up live** (the ticket's "numbers in the
   PR"). On a Windows laptop with no discrete GPU, with `onnxruntime.dll`
   (Microsoft's CPU build, 1.28 or later) next to `meet-ai.exe`: download
   Parakeet in Settings, pick it, record a 10-minute call with both sides
   talking. Expected: live lines keep appearing a short while after each
   pause and the delay does not grow over the 10 minutes (measure the delay
   at the start and at the end). Also run item 1 there and record the RTF
   (measure it; no number is assumed here). Write the CPU model, core count, RAM and
   RTF in the PR. Skipped: needs that laptop.
3. **Same on Linux** (x86_64, no GPU): ONNX Runtime is linked into the binary
   there, so no DLL step. Same expectations. Skipped: needs a Linux machine.
4. **Windows without the DLL** (every Windows build until the installer
   ships it, see Follow-ups): open Settings, Speech. Expected: the Parakeet
   row is disabled with "Not ready on this system yet: this copy of meet-ai
   does not include the ONNX Runtime library Parakeet runs on." and offers no
   Download. With `"engine": "parakeet"` set by hand in config.jsonc, a
   recording carries on and live transcription reports "config asked for the
   Parakeet engine but the Parakeet engine needs ONNX Runtime
   (onnxruntime.dll), which is not installed at <path>"; no crash, and
   Windows' own `System32\onnxruntime.dll` is never loaded. Then put
   Microsoft's CPU `onnxruntime.dll` (1.28 or later) next to `meet-ai.exe`
   and reopen Settings: the row offers the download. Skipped: needs Windows.
5. **Download in the app** (any OS, signed build): Settings, Speech,
   Parakeet, Download. Expected: one bar for all three files, "Checking the
   file is the right one" while each is hashed, then Parakeet becomes
   pickable. Kill the app mid-download and download again: it resumes from
   the `.part`. Skipped: needs the running app and a 670 MB download.
6. **Settings, About** shows "Parakeet speech model: parakeet-tdt-0.6b-v3 by
   NVIDIA, used under the Creative Commons Attribution 4.0 licence
   (CC-BY-4.0)..." and the link opens the model card in the browser.
   Skipped: needs the running app.
7. **macOS app size**: ONNX Runtime is linked statically on macOS and Linux;
   compare the release bundle size before and after this change (measure
   it). Skipped: no release builds here.

## Follow-ups (not in this ticket)

- Ship `onnxruntime.dll` in the Windows installer: a release.yml step plus a
  `tauri.windows.conf.json` resource next to the exe, beside TUR-61's
  `vulkan-1.dll`. Until then Settings shows Parakeet as not ready on Windows
  and recording reports the missing DLL (item 4).
- Consider Parakeet for `Auto` on Windows/Linux when TUR-61's hardware check
  finds no GPU. Left out on purpose: TUR-61 owns the `Auto` rule.
- Live partials (a volatile guess while someone is still talking) are off for
  Parakeet; lines appear when the speaker pauses, as with whisper's default.
