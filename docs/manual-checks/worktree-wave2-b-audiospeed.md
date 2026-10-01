# Manual checks: Wave 2, Phase 5 (audio path speed)

None of this could run headless: it needs a real mic, system audio, a model or the sidecar.

## 1. Mic and system recording, same output as before
- Run: record 30 s of mic and 30 s of system audio (play a video), stop, open the meeting folder.
- Expect: WAV length and content as before the change (compare with a recording from `main`); no clicks or gaps.
- Why skipped: needs microphone, system-audio permission and a running app.
- Touched: `crates/audio/src/mic.rs`, `macos/tap.rs` (worker loop buffer reuse).

## 2. System tap callback (lock removed)
- The real-time callback now uses a `RefCell` plus direct channel reads, not a `Mutex` and a staging copy.
- Expect: channel order correct (left and right not swapped), frame counts right, no dropouts while the CPU is busy.
- Why skipped: needs a live Core Audio tap.

## 3. CPU use and dropouts
- Run: a 30 min two-sided meeting; watch CPU in Activity Monitor, check the transcript for gaps.
- Expect: equal or lower CPU than `main`; no dropped audio.
- Why skipped: needs a real meeting.

## 4. Live partials with Whisper
- Expect: volatile (grey) text matches what it showed before; settled lines identical.
- Why skipped: needs a downloaded model (not allowed here).

## 5. Apple live path
- Expect: speech reaches the sidecar unchanged; lines appear as before. Bytes are covered by a unit test only.
- Why skipped: needs the sidecar and speech permission.

## 6. Model download retry
- Covered by in-process HTTP server tests. To confirm for real: start a download, cut the network for a few seconds, restore; it should resume and the checksum should pass.
- Why skipped: never download a model here.

## Known gaps and decisions
- Still one lock per chunk in the mic and tap worker loops (shared with the checkpoint thread), and in the Apple path (`heard` timeline). Removing them needs a redesign.
- `Tee::offer` still makes one `to_vec` per chunk; the vector moves through the channel to `stt` and `src-tauri`. Changing it changes the `TeeFeed` API.
- Existing quirk, left as is: `SpanAssembler` lead padding depends on block size. Blocks of 300 samples or fewer lose one frame of lead padding. Pinned by a test. Decision for Shann: fix it later (changes output).
- `SpanAssembler` moved from `session.rs` to `span_assembler.rs` (re-exported) to keep `session.rs` from growing.
- `Resampler::process` is now test-only; `process_into` is the real path.
- `modelfetch` got the tokio `time` feature (already a dependency, no new crate).
