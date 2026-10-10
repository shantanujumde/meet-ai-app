# Manual checks: TUR-175

Five transcript storage edges, each covered headless:

- `SharedSink` recovers a poisoned lock, so a panic on one track does not stop
  the other (`stt` `session::shared_sink::tests`).
- `transcribe_meeting` replaces `transcript.md` through
  `meeting_format::write_atomic` instead of appending, so a second run leaves
  one transcript (`stt` `transcribe::tests::running_again_replaces_...`).
- `MarkdownSink`'s docs now say it buffers, and that only `src-tauri`'s
  `Durable` wrapper makes live recording crash-safe (docs only).
- One transcript line parser, `meeting_format::transcript::parse_line` /
  `parse_hms`, used by `store::transcript` and the Start Work excerpt
  (`prompts` `excerpt_keeps_exactly_the_lines_the_meeting_view_parses`).
- `index.db` compares each meeting's total file size beside its newest
  modified time (schema 4), so an edit inside one coarse clock tick is
  indexed (`store` `index::tests::an_edit_that_keeps_the_modified_time_...`).

## Run by hand

1. Put the meetings folder on an exFAT USB stick (2 s timestamps). Open a
   meeting, type "kangaroo" in the notes, wait for the autosave, then within
   a second type "wombat" and wait again. Search for "wombat".
   Expect: the meeting is found.
   Why skipped: needs the running app and an exFAT volume.
2. After updating, launch the app once with an existing meetings folder.
   Expect: search works as before; `index.db` is rebuilt once (schema 3 to
   4), so the first launch reads every meeting.
   Why skipped: needs the running app.

## Known

- An edit that keeps both the size and the modified time (one letter
  swapped for another, inside one 2 s tick) is still missed until the next
  change or rebuild. Treating any recent time as changed would re-read
  meetings that did not change.
- TUR-152's finding (fixture `2026-09-02-1000-retro/transcript.md` lines not
  indexed) was already explained by TUR-166 (#195): the missing line has the
  speaker `Priya`, which SPEC §3.4 does not parse, and
  `a_line_with_a_speaker_the_spec_does_not_allow_is_not_indexed` holds it.
