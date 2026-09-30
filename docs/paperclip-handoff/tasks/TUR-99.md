# TUR-99 — Phase 3a — crates/store: read and write the meetings folder

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **todo** |
| Priority | high |
| Owner | Rune |
| Created | 2026-09-28 07:10 UTC by Alen |

## Description

`crates/store` is 75 lines of constants, an error type and two tests. Its own header says "Phase 3 territory, nothing is implemented yet". The app currently writes the meeting folder from `src-tauri/src/recording.rs` and reads it back ad hoc. Phase 3 is where that becomes one place.

#### Scope

Read and write every file in SPEC §3.1, in both directions:

- `meeting.md` (§3.2) — frontmatter plus the Summary / Decisions / Action Items / Open Questions sections.
- `transcript.md` (§3.4) — the strict, parseable format the `stt` sink already writes.
- `notes.md` — the user's own notes.
- `TICK-NNNN.md` (§3.3) — tickets.

#### Two rules that are easy to break by accident

**Unknown frontmatter keys must survive a round trip.** An agent, a future version, or the user may put keys there that this code has never heard of. Reading and writing a file back must not drop them.

**A malformed file must not take the app down.** SPEC §7: the parser tolerates a missing section, and the UI shows a "needs attention" badge instead of failing. An agent writing sloppy markdown is an expected condition, not a bug report.

#### Gate

- Round-trip tests over a fixture folder, including one file with unknown frontmatter keys, one with a missing section, and one that is plain broken.
- The broken one loads, is flagged, and does not stop the other two from loading.
- `just check` clean, including the Windows cross-check — `store` is explicitly on the list of crates that must stay free of mac-only code (SPEC §8.2).

## Comments (1)

### Alen · 2026-09-28 12:36 UTC

> Phase 3 starts here, and it starts with you.
>
> Phase 2b (TUR-92 and its children) is parked behind a gate only the user can run — a signed bundle, a real mic, real system audio, real Privacy toggles. That gate does not block anything in Phase 3, and there is no reason for three idle engineers to wait on it.
>
> TUR-99 is the foundation the other three Phase 3 tickets sit on, so it goes first and alone. The ticket text is already specific; the two things I would not let slide:
>
> - Unknown frontmatter keys survive a round trip. A future version or an agent will put keys in there that your code has never seen, and dropping them silently is data loss.
> - A malformed file is an expected condition, not a crash. It loads, gets flagged, and the files either side of it still load.
>
> One coordination note: the workspace is shared and runs are serialized, so commit in small pieces rather than sitting on a large uncommitted tree. TUR-100, TUR-101 and TUR-102 are now blocked on this ticket and will wake when you close it — Vox and Nia are waiting on your file format, so if the store API shape changes from what the ticket implies, say so in a comment rather than only in code.
