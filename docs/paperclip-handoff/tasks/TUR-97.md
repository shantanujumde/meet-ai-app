# TUR-97 — Phase 2b-4 — a recording killed mid-meeting still leaves usable files

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | medium |
| Owner | Rune |
| Created | 2026-09-28 07:09 UTC by Alen |
| Parent | [TUR-92](TUR-92.md) Phase 2b — wire the real recorder and live transcription behind the Record button |
| Blocked because | Waiting on TUR-95 (blocked) |

## Description

SPEC §5 puts `survives kill -9` in the Phase 0 gate, and `meet-rec` was built for it with incremental writes. Once the recorder moves into the app (2b-1, 2b-2) that guarantee has to be re-proven at the new boundary — the app has a webview, a tray, a global shortcut and a Tauri runtime between the user and the WAV writer, and any of them can be where the process actually dies.

#### Scope

- `kill -9` the app mid-recording. Both WAVs must be playable up to the moment of the kill, and `segments.json` readable.
- Same for the other ways a laptop ends a recording: closing the lid, logging out, the machine sleeping, and a plain force-quit.
- On next launch the app finds the interrupted meeting and opens it like any other, with whatever audio and transcript survived. No repair step, no error screen, no folder the user has to go clean up in Finder.
- Decide and write down what a partly-written meeting is called in the UI. It is not a normal finished meeting and it should not silently pretend to be one.

#### Gate

A scripted run: start a recording, wait a minute, `kill -9`, relaunch, open the meeting, play both tracks and read the transcript. Repeated at least once after an hour-long recording, because a WAV header that only breaks past a size threshold is exactly the failure this ticket exists to catch.
