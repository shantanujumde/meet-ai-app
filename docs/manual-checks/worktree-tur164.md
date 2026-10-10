# Manual checks: TUR-164 (timestamps through the anchors, live and batch)

What changed: a transcript line's time is now its WAV position placed through
`segments.json` (`stt::segments::place`): each segment's own start, then the
straight line between the checkpoint anchors around the frame, or the nominal
rate past the last one (SPEC A37). The batch path (`transcribe_meeting`, used
after Stop when live transcription is off) reads `segments.json` for it. The
live session reads the copy the recorder publishes on each tee feed every time
it writes the file (`audio::tee::TeeFeed::timeline`), through the same
function. The resamplers are not touched.

Tested headless: a simulated 45-minute pair of channels 1.001 apart (500 ppm
each way) stays within 3 ms of the host clock after mapping, and misses the
gate by over 1 s without the anchors
(`segments::drift_tests::a_45_minute_pair_of_clocks_1_001_apart_stays_inside_the_gate`);
batch and live give the same second for the same WAV position across drift and
a device-switch gap
(`placement::tests::batch_and_live_place_the_same_frame_at_the_same_second`);
the app's live wiring places lines by what the recorder publishes
(`live_transcript::tests::a_live_line_is_placed_by_the_segments_the_recorder_publishes`).

## Run by hand

1. The 45-minute two-clock recording (the ticket's "done when").
   - Setup: signed build, built-in microphone as input, a USB speaker or
     AirPods as output, so the two tracks run on two different crystals. Put
     the USB speaker next to the Mac (with AirPods, hold one bud against the
     Mac's mic). Make a 45-minute audio file with a short spoken marker
     ("marker one", "marker two", ...) every 5 minutes and silence between.
   - Run: start a recording, play the file through the output device for the
     whole 45 minutes, stop. Do it once with live transcription on and once
     with `transcription.live: false`.
   - Expect: each marker appears on an `Others:` line (the system track) and a
     `You:` line (the mic heard the speaker) with the same `[HH:MM:SS]`, or one
     second apart where a marker straddles a second boundary, from marker one
     to marker nine. For the 200 ms number: in an audio editor, find the
     sample where the last marker starts in `mic.wav` and in `system.wav`,
     and compare the two through `segments.json`
     (`stt::segments::SegmentsTimeline::frame_to_sec`): the difference should
     be under 200 ms. `cargo run -p audio --bin drift-check -- <meeting>/audio/`
     still reports each channel's raw drift against the host clock; a value
     over 200 ms there is now corrected in the transcript, so note it, but it
     is not a failure of this check.
   - Why skipped: needs real devices with two clocks, a signed build and 45
     minutes of real time.
2. A device switch mid-meeting, batch path.
   - Run: with `transcription.live: false`, record 5 minutes with spoken
     markers every minute, connect AirPods (or switch the default output) at
     minute 2, stop.
   - Expect: markers after the switch keep their real minute in
     `transcript.md` (before this change they came out a few hundred ms to a
     second early, by the gap at the switch). `segments.json` shows the second
     segment with `"reason":"default_output_device_changed"`.
   - Why skipped: needs the hardware and the signed app.
3. Pause and resume, both paths.
   - Run: record 1 minute, pause for 2 minutes, resume, say "after the pause",
     stop. Once with live on, once with it off.
   - Expect: "after the pause" is stamped about 3 minutes in (1 minute + the 2
     minute pause), in both runs, not 1 minute in.
   - Why skipped: needs the running app and a microphone.
