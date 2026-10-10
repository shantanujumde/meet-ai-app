# Manual checks: tur162

TUR-162 (audit AUD-4, AUD-6, AUD-11): recording crash safety.

The fix, all in `crates/audio`:

- **Headers never ahead of `segments.json`.** `AudioSource` gained
  `stop_capture` (stop, join the worker, fsync; header untouched).
  A stop, a pause and the first half of a reopen now end the segment through
  `session/finish.rs`: stop and fsync both channels, latch the close anchor,
  write `segments.json`, and only then patch the headers. A reopen does this
  before the new sources start, so the up-to-50 s wait for a new tap (30 s
  permission bound plus two 10 s first-buffer waits) no longer leaves
  headers ahead. If the system track will not finish at the stop,
  `segments.json` is still written (system frames up to the last checkpoint),
  the microphone is still finished, and the error comes back after.
  Abandoned sources (a failed start or reopen) are stopped without a header
  patch.
- **Late builds are torn down.** The tap's `Built` holds its Core Audio
  objects in guards (`macos/tap_guard.rs`: IO proc, aggregate device, process
  tap) and its worker in a `Worker` that joins on drop (`worker.rs`), so every
  `?` in `build` and every drop tears down what exists. The microphone's
  `Built` and `MicSource` do the same (`MicSource` has a halting `Drop`).
  Both starts go through `init_thread::run_bounded`: a build that finishes
  after the 30 s timeout is dropped on its own thread, which stops it.
- **Checkpoint retries.** A failed checkpoint is logged at warn and tried
  again at the next tick; the recording ends only at 6 failures in a row or
  30 s since the last good checkpoint (`session/retry.rs`).

Ran headless here: `cargo test -p audio` (all pass, including the new
`session::tests::crash_safety`, `init_thread`, `worker`, `session::retry` and
`mic` tests), `cargo clippy -p audio --all-targets -- -D warnings`,
`cargo fmt --all --check`, `cargo check --target x86_64-pc-windows-msvc -p
audio --tests`, `cargo test -p meet-ai --lib -- recording live_transcript`.
The crash-order tests were checked against the old order (a stub whose
`stop_capture` also patched the header): 5 of 7 failed, as they should.
Nothing below ran: no audio was recorded, no TCC dialog was shown, no app
was launched.

## Run by hand

Use a signed build of this branch on macOS 26+ with AirPods (or any second
output device).

1. **AirPods swap + `kill -9` (SPEC §5's gate).** Start a recording with
   something playing. Switch the default output to the AirPods, and while
   the new tap is still coming up (the first seconds after the switch),
   `kill -9` the app. Run `drift-check` on the meeting folder (or read both
   WAV headers and `segments.json`). Expected: neither header declares more
   frames than `segments.json` sums for that channel; segment 0 ends with a
   close anchor. Skipped here: needs real devices and a signed build.
2. **`kill -9` during stop.** Start a recording, press Stop, and `kill -9`
   the app within the first second. Expected: same invariant as 1; the
   recording opens and transcribes without the "invariant broke,
   extrapolating" warning in the log. Skipped here: needs the running app.
3. **System-audio TCC dialog answered late.** On a Mac where the
   system-audio permission has never been asked (`tccutil reset All
   pro.saleschat.meetai` beforehand, by hand), open onboarding's system-audio
   check and leave the dialog up for more than 30 s, then allow it.
   Expected: the check reports the timeout; within a second of allowing,
   the purple recording indicator in the menu bar goes out, and no
   `meet-ai system aggregate` device is left in Audio MIDI Setup. Skipped
   here: needs a person to answer the dialog.
4. **Microphone TCC dialog answered late.** As 3, for the microphone
   permission. Expected: after allowing, the orange microphone indicator
   goes out, and Activity Monitor shows no `meet-rec-mic-worker` thread
   using CPU (before the fix it polled every 2 ms until quit). Skipped here:
   same reason. Verify it: `cpal`'s `Stream` drop stops the audio unit on
   macOS (read in cpal 0.18.2 and coreaudio-rs 0.14.2); on Windows and
   Linux the stream drop was not read, so check the indicator there too if
   a late-answered prompt can happen.
5. **A disk hiccup does not end the recording.** Record into a meeting
   folder on a USB stick or an iCloud folder, and pull or throttle the disk
   for about a second mid-recording (or, on Windows, let an antivirus scan
   lock the folder). Expected: the log shows `checkpoint failed ... trying
   again at the next tick` at warn, then a normal `checkpoint at ...` line,
   and the recording goes on. Removing the disk for good ends the recording
   within about 30 s with "N checkpoints in a row failed". Skipped here:
   needs a removable or network disk and the running app.

## Notes for review

- Six fast failures at the 200 ms tick span about a second; failures that
  each block (a slow fsync) run into the 30 s limit instead. Both constants
  are in `session/retry.rs`.
- A system track whose header patch fails at a reopen still gets a fresh
  tap, as before; that tap appends at the old declared length, so the
  unpatched tail of the old segment is overwritten. Pre-existing, rare
  (the fsync just worked), left alone.
- `src-tauri/src/live_transcript/e2e.rs`'s fixture source (owned by
  TUR-148's area) was not touched: `stop_capture` defaults to `stop`, so that
  test fixture compiles and keeps its old order. Every real source (mic, tap,
  loopback) and the session's own stubs override it.
