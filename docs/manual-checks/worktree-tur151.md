# TUR-151: mic and system audio alignment (start, device switch, anchors)

All three parts are unit-tested with stub sources (`cargo test -p audio`):
`session::tests::align` (a late second source is padded by its frame-0 time;
a reopened segment is padded at its own start), `wav_writer` (a reopened
writer refuses `prepend_silence` and pads its segment head), `track` (the
reopen pad on a real `TrackWriter`) and `capture_clock` (every position names
the capture time of its frame at 48, 44.1 and 16 kHz, with a ring backlog and
the resampler's pending input). What needs real hardware is below; none of it
was run here (no signed build, no microphone or speakers this worker may
use, and `just rec` is off limits).

## Start alignment and the 200 ms drift gate

- Run: a signed build, `just rec --out <DIR> --duration 120` (or a 2-minute recording in
  the app) with speakers on, then `cargo run -p audio --bin drift-check -- <DIR>`
  and `cargo run -p audio --bin drift-check -- --audio <DIR>`.
- Clap once near the start, where both the microphone and the speakers
  (play a clap through them) are heard.
- Expected: `drift-check` passes the gate; the `--audio` lag is a constant
  (air plus output latency) of well under 200 ms, not hundreds of ms. In a
  wave editor, the clap lines up on both tracks to within tens of ms.
- Before the fix the system track's head-pad was short by every frame the
  microphone wrote while the tap was starting (often 100 ms or more).
- Why skipped: needs a signed build, a microphone and system-audio grant.

## Device switch (AirPods swap)

- Run: as above, and switch the default output (and/or input) device about
  30 s in, with a clap before and after the switch.
- Expected: `segments.json` has two segments; the pre-switch clap is still
  aligned on both tracks (nothing before the switch moved), and the
  post-switch clap is aligned too. The log shows "padding ... head with N
  frames" once per segment for at most one channel.
- Why skipped: needs real devices and a signed build.

## The microphone's capture time on macOS (verify it)

- The microphone now stamps each packet with `cpal` 0.18.2's `callback`
  instant, which its Core Audio backend builds from the input callback's
  `AudioTimeStamp.mHostTime` (read in the cpal source; not documented by
  cpal). That this is the first frame's capture time, like the tap's
  `inInputTime`, is not confirmed. A time more than 1 s from the host clock
  (or in the future) is ignored and the callback's own time less the packet
  is used instead.
- Run: the clap test above, twice (the microphone side is the one that
  changed).
- Expected: the clap lag between the tracks is the same with this change as
  the `--audio` estimate says, and stable across recordings.
- Why skipped: needs a microphone and a signed build.

## Windows and Linux loopback

- The loopback source and the microphone use the same capture-time path
  (`capture_clock`); covered by the `rust (windows | linux)` CI job, and by
  the loopback fake-backend tests here. A real recording with a clap, as
  above, on each OS is still needed.
