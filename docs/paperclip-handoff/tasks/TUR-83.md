# TUR-83 — cannot drag app

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Nia |
| Created | 2026-09-28 06:02 UTC by You (board) |
| Completed | 2026-09-28 06:46 UTC |

## Description

![](/api/assets/0d5f2f89-2062-4be0-9a1d-be731cfd3eb5/content)once the initila setup is done I cannot drag the app

## Commits that mention this task

- `c7c97a0` 2026-09-28 — TUR-83: fix window drag with data-tauri-drag-region

## Questions you answered

- **Q:** I changed how the app window is dragged (src/ui/Shell.tsx and src-tauri/capabilities/default.json). After finishing setup, can you click and hold on the dark title bar at the top of the window (on the app name or the empty space, not on the Record button) and drag it? Does the window move now?
  **A:** No — still stuck, do I need to restart it?
- **Q:** After a full restart (quit and reopen, or stop and rerun pnpm tauri dev), can you drag the window by its title bar now?
  **A:** Yes

## Comments (5)

### Nia · 2026-09-28 06:46 UTC

> Confirmed fixed — you reported dragging works now after a full restart. Closing this out.
>
> Summary for the record: the title bar was only using a CSS-only drag trick that Taul's macOS window does not honor. The real fix was adding Tauri's actual drag attribute to the title bar in `src/ui/Shell.tsx`, plus a permission (`core:window:allow-start-dragging`) that attribute needs in `src-tauri/capabilities/default.json`. Committed as c7c97a0 on `docs/releasing-guide`. The Record button was deliberately left out of the drag area so it still works as a button.

### Nia · 2026-09-28 06:42 UTC

> Yes — please fully restart the app, not just refresh or click away and back.
>
> Here's why: the fix needed a permission setting that gets baked into the app when it starts up. If the app was already open before the fix was installed, it's still running the old version without that permission, and no amount of clicking inside the window will pick up the change.
>
> How to restart, depending on how you're running it:
> - If you started it with the command `pnpm tauri dev` in a terminal, stop that (press Ctrl+C in that terminal), then run `pnpm tauri dev` again.
> - If you're using the built app (an app you double-click to open), fully quit it with Cmd+Q (not just close the window) and reopen it.
>
> Once it's back open, try dragging by clicking and holding on the title bar — the strip at the top of the window that says "meet-ai" — and moving the mouse. (Dragging the Record button itself won't work; that's expected, it needs to stay clickable.)
>
> Let me know if it still doesn't move after a full restart — if so, I'll dig further.

### Nia · 2026-09-28 06:29 UTC

> Re-checked after this re-wake: this is the same report I already fixed in commit c7c97a0 (data-tauri-drag-region on the titlebar in src/ui/Shell.tsx, plus the core:window:allow-start-dragging permission). I re-read Shell.tsx just now and the fix is still in place correctly. There is an open confirmation request on this issue ("Confirm the window can be dragged") still waiting for someone to actually try dragging the window post-setup - no response on that yet, so I am leaving this in review rather than redoing the same fix. No new code changes this heartbeat.

### Nia · 2026-09-28 06:23 UTC

> Fixed and committed (c7c97a0 on docs/releasing-guide): TUR-83 was caused by the titlebar relying only on CSS `-webkit-app-region: drag`, which does not actually move the window on Tauri's macOS WKWebView. The window was never really draggable through the custom titlebar chrome; it just happened to go unnoticed during onboarding because there was nothing to click there.
>
> What changed:
> - `src/ui/Shell.tsx`: added the `data-tauri-drag-region` attribute (Tauri's real drag mechanism) to the titlebar, the app title, and the spacer. The Record button is deliberately left out so it still responds to clicks.
> - `src-tauri/capabilities/default.json`: added the `core:window:allow-start-dragging` permission, which that attribute needs and which was missing (not part of `core:default`) - without it, dragging fails silently.
>
> Verified statically: `biome check`, `tsc --noEmit`, and `cargo check` all pass clean on the changed files.
>
> Gap: I could not verify this by actually dragging the running window this heartbeat - launching `pnpm tauri dev` here hit a shared cargo build-directory lock held by another in-progress agent process, and I don't have a way to simulate a native mouse drag gesture from this session. The fix follows Tauri's documented working pattern exactly, but it needs a real click-and-drag check before this is closed out.
>
> @Tess (agent://23653fce-5de4-47b8-bc18-f2da0505fa01): could you take this for live verification? Repro steps:
> 1. `pnpm tauri dev` (or run the built app) from a clean checkout of this branch.
> 2. Get past onboarding to the main window (the one with the Meetings sidebar and Record button, shown in the reporter's screenshot).
> 3. Click and drag on the titlebar - on the app title text, and on the empty space to the left of the Record button - and confirm the window actually moves.
> 4. Also confirm the Record button and Settings link still click normally (the fix intentionally excludes the button from the drag region).
>
> Marking this in_review pending that check.

### Alen · 2026-09-28 06:06 UTC

> Routed to you (Nia) from TUR-84 — you fixed the window chrome in TUR-42, so this is the same surface. After initial setup finishes, the window can no longer be dragged. Likely the drag region is lost when the view switches away from onboarding.
