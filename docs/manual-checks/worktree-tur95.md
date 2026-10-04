# Manual checks: TUR-95 (Windows: record the output device a call actually plays to)

These need a real Windows 10/11 PC with two output devices (laptop speakers
plus a USB or Bluetooth headset) and a real call, so none were run here. They
close check 6 of `worktree-tur37.md`.

What is covered without a device, in every `cargo test -p audio` on macOS,
Windows and Linux (`crates/audio/src/platform/windows_render_choice.rs`):

- The endpoint choice over fake session lists: nothing playing → the default;
  one non-default device playing → that device; the default playing, or
  several non-default devices playing → the default; meet-ai's own sessions
  (its silence keepalive) and the system-sounds session (pid 0) do not count;
  an inactive session does not count; no default endpoint → only a single
  playing device is an answer.
- The choice fed through the unchanged TUR-37 switch policy (`DeviceWatch`,
  read every 2 s, switch after 2 agreeing reads): a call starting on the
  headset is followed about 4 s later and dropped about 4 s after it ends; a
  sound seen by one read only is not followed; the default starting to play
  takes the recording back.
- Only a non-default endpoint is opened by id; the default is still opened
  through `cpal`'s default device, so Windows keeps rerouting it.

On the `rust (windows)` CI job only: the unit test in
`crates/audio/src/platform/windows/render_in_use.rs` runs the real WASAPI
session read and checks it does not panic (a runner usually has no endpoint).
On this Mac the Windows files were only type-checked and linted
(`cargo clippy --target x86_64-pc-windows-msvc -p audio --all-targets`, with
and without `--features stub-audio`).

Not measured, so measure it on the PC: whether a call app's render session
stays `Active` through long silences on the call (if it drops to `Inactive`
the recording falls back to the default after two reads, and back again when
the call is heard). Log lines in the app's `meet-ai.log` tell which device was
opened.

Setup for every check: as in `worktree-tur37.md` (Windows 10 1703 or later or
11, the repo at this branch, `cargo build -p audio --bins`, `meet-rec` and
`drift-check` in `target\debug\`), or the app built from this branch with its
Record button.

## 1. Call on a headset that is not the default output

1. Laptop speakers as the Windows default output (Settings → System → Sound).
   Connect a headset but keep it **not** default.
2. Join a Teams desktop or Zoom call with someone talking, and in the call
   app's audio settings pick the headset as its speaker.
3. `meet-rec --out C:\tmp\tur95-headset --duration 90`.
4. Expected: `system.wav` has the other side's voice from the start (the
   first read of a recording is taken as is), one segment whose
   `sys_device_rate` is the headset's rate. In the app's log:
   `system loopback: following {…}, which other apps play to`.
5. `drift-check C:\tmp\tur95-headset`: exit code 0.
6. Again, but start `meet-rec` (90 s) first and join the call at about 0:20.
   Expected: a second segment with reason `default_output_device_changed`
   about 2 to 4 s after the call's audio starts, on the headset's rate, and
   the other side's voice in `system.wav` from then on.

## 2. The default still wins when it is playing

1. As in 1, but also play a YouTube video on the laptop speakers (the
   default) for the whole recording.
2. `meet-rec --out C:\tmp\tur95-both --duration 60`.
3. Expected: one segment only; `system.wav` has the video, not the call (the
   default wins ties, by design). No `following` line in the log.

## 3. Call ends, back to the default

1. As in 1, with `--duration 120`. Leave the call at about 1:00 and start a
   video on the laptop speakers.
2. Expected: a segment back to the default device about 4 s after the call
   ends, and the video in `system.wav` from then on. `drift-check` exits 0.

## 4. Headset unplugged while followed

1. As in 1. At about 0:40 unplug or disconnect the headset.
2. Expected: the recording does not end; a new segment on the default device
   within about 6 s (the log may show `… is gone; recording the default` if
   the reopen raced the unplug), and `drift-check` exits 0.

## 5. Nothing playing and the default only (no regression)

1. Re-run `worktree-tur37.md` checks 2 and 3 on this branch. Expected: the
   same results as there; no extra segments while nothing plays.
