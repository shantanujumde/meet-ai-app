# TUR-84 — assign tasks

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-28 06:04 UTC by You (board) |
| Completed | 2026-09-28 06:07 UTC |

## Description

check all the un assigned tasks and assign them to the respective agents

## Comments (3)

### Alen · 2026-09-28 06:07 UTC

> ## Assigned all 7 unassigned tasks
>
> Routing follows what each agent already owns.
>
> | Task | Owner | Why |
> |---|---|---|
> | TUR-78 Audio permission not detected | Rune | Owns the macOS capture-permission path (TUR-3, TUR-10) |
> | TUR-79 App always starts at welcome screen | Nia | Owns the app shell and onboarding (TUR-17) |
> | TUR-81 Engine not detected, no guidance | Vox | Owns meet-stt engine readiness (Phase 1) |
> | TUR-82 Option to change directory | Nia | Settings live in the app shell |
> | TUR-83 Cannot drag app after setup | Nia | Same window chrome she fixed in TUR-42 |
> | TUR-19 "Keychain Not Found" dialogs | Alen | Runtime/environment, same as TUR-20 |
> | TUR-73 Watchdog gets one write per run | Alen | Paperclip platform bug, not product code |
>
> Each delegated task has a short scope note in its own thread. The board is now fully assigned — zero unassigned tasks.
>
> Two things worth flagging:
>
> - **TUR-81 crosses two people.** Engine detection is Vox's, but the "tell the user how to fix it" wording shows up in Nia's app shell. I asked Vox to pull Nia in if the UI needs more than message text.
> - **TUR-73 is a platform report, not a code fix.** It sits alongside TUR-28 as something Paperclip itself has to change, so I kept it with me rather than sending it to a product engineer.
>
> Aria (designer) has no work. The current queue is all bugs, so there was nothing design-shaped to route.

### You (board) · 2026-09-28 06:05 UTC

> start

### Paperclip (automatic) · 2026-09-28 06:04 UTC

> Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.
