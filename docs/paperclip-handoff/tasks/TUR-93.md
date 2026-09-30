# TUR-93 — Create Next set of tickets

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-28 07:06 UTC by You (board) |
| Completed | 2026-09-28 07:11 UTC |

## Description

_No description._

## Comments (1)

### Alen · 2026-09-28 07:11 UTC

> Nine tickets created, covering the rest of Phase 2 and all of Phase 3.
>
> ## What I found first
>
> The recorder, both transcription engines and the model downloader are all built and tested. `crates/audio` is ~4,500 lines, `crates/stt` ~5,000. None of it is connected to the app. `src-tauri/src/recording.rs` is still the TUR-17 stub with `stub: true` hardcoded, which is why TUR-90 produced two empty meeting folders.
>
> The specific reason it was never connected: the entire capture loop is `fn record()` **inside the binary** `crates/audio/src/bin/meet-rec.rs:380`. It is private, it blocks, and it stops on a `--duration` flag. There is no way for the app to start and stop it. That is why TUR-94 is first — it is the thing standing between a finished recorder and a working Record button.
>
> `crates/store`, `detect`, `calendar` and `prompts` are still 46–75 line stubs. Phases 3, 4 and 5 have not been started.
>
> ## Phase 2b — finishing the app (children of TUR-92)
>
> | | | |
> |---|---|---|
> | TUR-94 | Lift the record loop into `crates/audio` as a start/stop session | ready |
> | TUR-95 | Put the real recorder behind the Record button | after 94 |
> | TUR-96 | Live transcript on screen and into `transcript.md` | after 95 |
> | TUR-97 | A killed recording still leaves usable files | after 95 |
> | TUR-98 | The 45-minute gate run in the app | after 95, 96 |
>
> TUR-98 also closes **TUR-71** and **TUR-7**, which have been stuck for one reason: no way to make a real recording without dropping to the CLI. I linked both to it, and TUR-6 to TUR-92, so the board now shows what actually unblocks them.
>
> ## Phase 3 — store and index
>
> | | | |
> |---|---|---|
> | TUR-99 | `crates/store`: read and write the meetings folder | after the gate |
> | TUR-100 | Folder watcher, with self-write suppression | after 99 |
> | TUR-101 | SQLite FTS5 index and search | after 99, 100 |
> | TUR-102 | Tickets in the UI, including manual create | after 99 |
>
> ## One judgement call
>
> I held all of Phase 3 behind the TUR-98 gate rather than opening it for work now. SPEC §5 says it plainly — miss a gate, stop, do not stack work on a broken layer — and Phase 2's gate has not been run. If you want Phase 3 started in parallel anyway, say so and I will drop the blockers; TUR-99 in particular has no real dependency on the recorder beyond the file formats.
>
> Nothing is assigned yet. TUR-94 is ready to pick up.
