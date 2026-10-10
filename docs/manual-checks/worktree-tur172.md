# Manual checks: tur172

TUR-172: less work on the backend's hot paths. Three Settings commands and
"Send a test reminder" read config on the blocking pool; the reminder loop
measures its wake gap from the end of a tick; `Live::get` reads with its lock
released; a usable `meet-stt --probe` is kept between recordings; the WAV
writer buffers samples (64 KiB) and syncs the header with `sync_data`;
`segments.json` is compact; the process poll keeps only meeting apps and reads
every 30 s while recording; on Linux one PulseAudio connection, subscribed to
server events, feeds the default-device watch.

## Measured here (macOS, APFS, debug build)

- WAV append, 120 checkpoints of 234 chunks of 341 frames (a temporary test,
  deleted): per chunk 14.2 to 14.9 µs before, 8.6 to 8.7 µs after. Syscalls
  per chunk per channel: an `lseek` and a ~700-byte `write` before; after, one
  64 KiB `write` per ~96 chunks plus one at each checkpoint. The checkpoint
  itself took 7.5 to 7.7 ms both ways (the sync dominates).
- Syncs per checkpoint: still 6 (owner decision A1: keep every sync). The
  header sync is now `sync_data`. On macOS Rust's `sync_data` and `sync_all`
  both use a full sync, so the gain there is on Linux; verify it with
  `strace -c` on a Linux recording.
- `segments.json` with 2,900 anchors (a 4-hour meeting): 491,972 bytes pretty,
  300,447 bytes compact (test `segments_json_is_compact_and_reads_back`).

## Run by hand

1. Record twice in a row with the Apple engine (signed build, macOS 26).
   Expect: the second start logs "opening live transcription" without a
   `meet-stt --probe` process (`ps -ax | grep 'meet-stt --probe'` during the
   start shows none the second time).
   Why skipped: needs a signed build, the mic permission and the running app.
2. Sign in to a slow Google or Microsoft calendar (or block the network so the
   read takes over 20 s), leave the app open a few minutes.
   Expect: the log does not say "the clock jumped" on every tick; it says it
   only after a real sleep.
   Why skipped: needs a signed-in account and the running app.
3. Linux (PipeWire or PulseAudio): record, then plug in a headset or change
   the default output in the sound settings.
   Expect: the recording follows the new device within a few seconds, as
   before; `pactl list clients | grep -c meet-ai` stays at 1 during the
   recording instead of a new client every tick. Verify it: that a default
   sink change reaches the app as a server change event (it re-reads on
   server changes and on sinks or sources added or removed).
   Why skipped: needs a real Linux machine with a sound server.
4. Linux: restart the sound server (`systemctl --user restart pipewire-pulse`)
   mid-recording.
   Expect: the watch reconnects within about 2 s and the recording goes on.
   Why skipped: as 3.
5. Zoom open with a call, start recording, stop after a minute.
   Expect: no "Record this Zoom call?" prompt at Stop (the slowed poll still
   marks Zoom as seen during the recording).
   Why skipped: needs a meeting app and the running app.

## Follow-ups

- Fewer syncs per checkpoint: the header patch's own sync could be left to
  the next checkpoint's data sync (6 to 4 syncs), at the cost of losing up to
  two checkpoints, not one, on a power cut. Not done (A1).
