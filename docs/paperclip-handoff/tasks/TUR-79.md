# TUR-79 — The app always starts with welcome screen

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Nia |
| Created | 2026-09-28 05:59 UTC by You (board) |
| Completed | 2026-09-28 12:25 UTC |

## Description

If the initial setup is done the app should start directly no need to show setup screen again

![](/api/assets/0bcd16af-1ba6-446b-8b9c-7d14e6563a0f/content)

## Commits that mention this task

- `eea3a42` 2026-09-28 — TUR-79: add a real-disk round trip test for the onboarding flag
- `c3d18f9` 2026-09-28 — TUR-79: don't trap a finished user on a leftover setup URL

## Questions you answered

- **Q:** When the setup screen came back, had you already clicked all the way through every step to the final "Done" button, or had you closed the app before reaching that last step?
  **A:** Yes, I clicked Done (or Skip setup) and finished the wizard
- **Q:** After rebuilding the app fresh (not the build the earlier screenshot came from), does setup still show again on the next start?
  **A:** No, it now goes straight to the meeting list

## Comments (5)

### Nia · 2026-09-28 12:25 UTC

> Confirmed fixed on a fresh rebuild: setup no longer reappears once completed, launch goes straight to the meeting list. This matches the c3d18f9 fix (skip onboarding once `onboarding.json` records `completedAt`, checked on every app start via `Bootstrap` in `src/App.tsx`), verified end-to-end with a real on-disk save/reload test. Closing as done.

### Nia · 2026-09-28 07:04 UTC

> Dug further since the earlier fix (c3d18f9) doesn't match what you saw — you confirmed you clicked all the way through to Done.
>
> What I checked, end to end:
> - How the app saves "setup finished": writes a small file (onboarding.json) inside your Meetings folder.
> - How the app reads it back on start: loads that file and, if it says finished, sends you straight to the meeting list.
> - The screen logic that decides whether to show setup: I re-read it line by line. It correctly skips setup once that file says finished.
>
> I also wrote a new test that does the actual save-then-reload on a real file on disk (the old tests only checked the save/load code in memory, never a real file) — it passes. So I cannot find a bug in the save-and-remember logic as it stands right now.
>
> My best guess: the screenshot you shared may have been from a build made before the earlier fix (c3d18f9) went in, rather than from a fresh one after it.
>
> Could you try this on a freshly rebuilt app and let me know: does setup still come back? If yes, one more detail would help a lot: were you closing the app with Cmd+Q (fully quit) or just closing the window?

### Nia · 2026-09-28 06:24 UTC

> The cargo test compile is running in the background (Rust cold compiles can take a few minutes); I'll wait for it to finish rather than poll. While it runs, here's where things stand:
>
> I've traced the full persistence path end-to-end and it checks out. The "setup done" flag is saved by the Rust backend to `~/Meetings/.app/onboarding.json` (`src-tauri/src/onboarding.rs`), and read back on every app start through `state()`, which the frontend calls via `loadOnboarding()` in `src/state/app.ts`. `src/App.tsx`'s `Bootstrap` component reads that value and redirects away from any onboarding URL once `completedAt` is set — that's the fix already committed as `c3d18f9`. The existing test `someone who has finished onboarding gets the meeting list, with its empty state` in `src/App.test.tsx` covers exactly the cold-start case (fresh hash, no leftover URL, onboarding already marked done), and I just reran the whole frontend suite — all 5 tests pass. So the previously committed fix isn't just patching the background-window edge case; it also correctly handles a genuine fresh launch.
>
> I'll confirm the Rust-side file read/write tests too once the compile finishes, then close this out.

### Nia · 2026-09-28 06:19 UTC

> Looked into this. Short version: I found and fixed one real bug, but I want to check my read of the screenshot against what you actually did before I call it fully closed.
>
> **What I found and fixed**
>
> The app has logic that sends an unfinished user *into* the setup wizard, but nothing that sends a finished user *back out* if they're still sitting on a setup screen URL. Concretely: the menu-bar app keeps running in the background even after you close the window (that's deliberate, so ⌘⇧R still works). If you reopen it, it just re-shows the same window wherever it was left — including mid-setup, if that's where it was sitting. Nothing checked "wait, has this person actually finished setup already?" and sent them onward. I added that check, plus a test that reproduces the exact scenario (start on a leftover setup URL with setup already marked done, confirm it lands on the meeting list instead). Committed as c3d18f9.
>
> **One thing I couldn't fully confirm**
>
> Your screenshot shows step 2 of 4 ("Let meet-ai hear your Mac", the audio-permission check), not the very first welcome screen. That step can currently only ever say "not checked" — the real check (a short tone played and listened for) isn't built yet, so it's not a sign that anything's wrong, and it's still safe to click Continue past it. If you always click Continue → Continue → Done through all 4 steps and it *still* comes back to setup next time, that would point to a second, different bug I haven't found yet (and I'd want to know that so I can dig further). If instead you sometimes leave the app partway through — closing the window before reaching the last "Done" step — then what you were hitting is exactly the bug above, and it should be fixed now.
>
> Could you confirm which of those matches what you saw? That tells me whether this is fully closed or whether there's more to chase.

### Alen · 2026-09-28 06:06 UTC

> Routed to you (Nia) from TUR-84 — the app shell and onboarding are yours from TUR-17. Setup completes, but the welcome/setup screen shows again on every launch. Find where 'setup is done' is meant to be persisted and why it is not read back on start.
