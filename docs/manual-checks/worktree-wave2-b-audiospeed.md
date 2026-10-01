# Manual checks: Wave 2, Phase 5 (audio path speed)

None of this could run headless: it needs a real mic, system audio, a model or the sidecar.

## Results, 2026-10-01 (signed bundle of `main` at 3f468ab)

How it was run: 4 recordings of 44–70 s. During each, a 33 s test sound played through the speakers: 5 s of 440 Hz on the **left channel only**, 5 s of 880 Hz on the **right only**, 2 s of silence, a `say` voice reading 4 sentences, then 5 s of 1000 Hz on both channels. A script then checked `system.wav`: the pitch of each tone, the loudness of each tone, and every sample against the pure-sine rule (`x[n+1] + x[n-1] = 2cos(w)·x[n]`). The script was first checked against the clean test sound (no false alarms) and against a copy with 10 ms cut out (caught it at the exact spot).

| Check | Result |
|---|---|
| 1. Output | ✅ 16 kHz mono, same format as the Sep 30 build. Mic and system lengths within 50 ms of each other and of the actual time. |
| 2. Tap callback, channels | ✅ All 4 runs: the left-only and right-only tones arrive at exact pitch (440.0 / 880.0 Hz) and the same loudness (within 5%). No channel lost or misread. |
| 2. Dropouts | ✅ 3 of 4 runs: 0 glitches. ⚠️ Run 2 (Whisper, first model load of the session, 6 s of Metal setup): extra sound mixed on top of the tone for about 75 ms at 4.68 s. No samples lost or repeated (no phase jump, same loudness after), so probably an outside sound, not the tap. The Whisper repeat had no glitch, but the model was already loaded, so the cold load was not re-tested. |
| 3. CPU | ⏳ Short runs only: about 4% average, peak 14.7% (Whisper) / 5.2% (Apple). The 30 min run is still open. |
| 4. Whisper live partials | ✅ Grey text first, then settled. Test voice word for word with `small.en-q5_1`. |
| 5. Apple live path | ✅ Test voice word for word, split into slightly shorter lines than Whisper. |

Found along the way:
- `config.jsonc` with `"engine": "whisper"` and no `model` asks for the default `large-v3-turbo-q5_0`. With only `small.en-q5_1` installed, live transcription fails with "no model is downloaded yet", which is misleading because one is. The message should name the missing model.
- The log file is cut back to empty while the app runs. The log plugin uses its defaults in `src-tauri/src/lib.rs` (small size limit, one file kept), and Whisper writes about 70 lines on every model load, so a few recordings wipe earlier lines. That makes a user-sent log useless. Consider raising the size, keeping old files, or turning `whisper_rs` down to WARN.

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
- **Run 2026-10-01 on the current signed build** (`target/release/bundle`, built with #28):
  - Resume from a leftover `.part`: passed. Download started at 54.1 MB and carried on from there, not from zero.
  - 30 s Wi-Fi cut at 74.7 MB: the download stalled, then carried on by itself 8 s after the internet came back, with no error and no Retry button. It finished at 190,098,681 bytes and the SHA-256 matched.
  - The log has no `download failed; retrying` line. The connection survived the cut (Wi-Fi came back with the same address), so the retry code still never ran for real. It stays covered by the in-process server tests only.
- For comparison, the Sep 30 build in `/Applications` (before #28) failed the same 30 s cut with a "network issue" error and a Retry button.
- Possible bug, found while reading the code: `download` (`crates/modelfetch/src/lib.rs`) sets only a connect timeout, with no timeout while bytes are arriving. A long drop may leave the download hanging with a frozen bar instead of failing and retrying. The 30 s cut should show which.

## Known gaps and decisions
- Still one lock per chunk in the mic and tap worker loops (shared with the checkpoint thread), and in the Apple path (`heard` timeline). Removing them needs a redesign.
- `Tee::offer` still makes one `to_vec` per chunk; the vector moves through the channel to `stt` and `src-tauri`. Changing it changes the `TeeFeed` API.
- Existing quirk, left as is: `SpanAssembler` lead padding depends on block size. Blocks of 300 samples or fewer lose one frame of lead padding. Pinned by a test. Decision for Shann: fix it later (changes output).
- `SpanAssembler` moved from `session.rs` to `span_assembler.rs` (re-exported) to keep `session.rs` from growing.
- `Resampler::process` is now test-only; `process_into` is the real path.
- `modelfetch` got the tokio `time` feature (already a dependency, no new crate).
