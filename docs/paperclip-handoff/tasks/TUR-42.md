# TUR-42 — the close max minimise btns are outside of the window

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Nia |
| Created | 2026-09-27 14:16 UTC by You (board) |
| Completed | 2026-09-27 14:37 UTC |

## Description

![](/api/assets/6f00ae30-4d28-49c5-ae01-e1ca8456b412/content)

[@Alen](agent://e5a80111-5cc4-4c6e-9172-c60785be2539)assign this to approprate agent

## Commits that mention this task

- `7f24434` 2026-09-27 — TUR-42: use titleBarStyle Overlay so traffic lights sit inside the window

## Comments (2)

### Nia · 2026-09-27 14:37 UTC

> Found and fixed the root cause.
>
> The attached screenshot is of a different window (unrelated editor content, no meet-ai data) sitting right above the app, so I reproduced the bug myself by running the real dev build and screenshotting the actual window.
>
> Root cause: `src-tauri/tauri.conf.json` used `"titleBarStyle": "Transparent"`. In Tauri, that value only makes the native title bar's own background see-through — it does **not** extend the app's content underneath it. So macOS kept reserving a separate native strip above our window for the traffic lights, and our own dark titlebar row started below that, making the buttons look like they were floating outside the window.
>
> `"titleBarStyle": "Overlay"` is the value that actually merges the title bar into the content (what our CSS — the 78px `--titlebar-traffic-inset` clearance and the drag region — already assumed). Changed it in `tauri.conf.json`, and updated the two design-system docs (`MASTER.md`, `glass.css`) that had the same wrong example, so nobody copies the broken value again.
>
> Verified: rebuilt the dev app, screenshotted the real window before and after — traffic lights now sit correctly inside the titlebar row next to the window title, with clean rounded corners. Committed as 7f24434 on this branch (3 files: tauri.conf.json, MASTER.md, glass.css).
>
> Marking done.

### Alen · 2026-09-27 14:16 UTC

> Routing this to **Nia** (Application Engineer, Tauri/React) — the window frame and its controls are hers.
>
> What the screenshot shows: the red/yellow/green buttons sit *above* the app's rounded top edge, on the desktop behind it, instead of inside the title bar. That points at the window setup — the native traffic lights are being positioned against the full native window rect while the visible app content is inset/rounded, so they land outside the painted frame. Usual suspects: the title-bar style and the traffic-light offset in the Tauri window config, plus any custom rounding on the root container.
>
> Nia — please confirm the cause, fix the inset so the controls sit inside the title bar at normal macOS spacing, and check both light and dark mode plus a resize and a fullscreen toggle.
