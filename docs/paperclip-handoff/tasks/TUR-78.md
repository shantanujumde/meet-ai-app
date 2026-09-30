# TUR-78 — Audio permission

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Rune |
| Created | 2026-09-28 05:58 UTC by You (board) |
| Completed | 2026-09-28 11:02 UTC |

## Sub-tasks

- [TUR-88](TUR-88.md) **done** — TUR-78 follow-up: verify the real permission-check round trip on real hardware

## Description

<img height="236" width="440" src="/api/assets/c7da8e00-6860-4da7-a74e-0a15623cc255/content" />

Audio permission issue even if the permission is given it is not able to detect it

## Commits that mention this task

- `4705583` 2026-09-28 — TUR-78: wire the onboarding permission check to the real chime probe

## Comments (7)

### Rune · 2026-09-28 11:02 UTC

> ## Closing — fix confirmed on real hardware
>
> TUR-88 (the real-hardware follow-up) is done. Recap of the full loop, now closed:
>
> **Root cause:** the onboarding permission check was a permanent stub (`permission::status()` always returned `Unknown, measured: false`) — it never read real audio hardware, so a real macOS grant had nothing to be detected by.
>
> **Fix (commit 4705583):** wired `permission::measure()` to two real checks — `check_mic()` (opens a real mic stream via cpal) and `check_system()` (plays a chime through the output device and confirms it's recovered from a live system-audio tap). Denied wins over granted; both channels must be granted for an overall "Allowed".
>
> **Real-hardware verification (TUR-88, done):**
> - Granted direction confirmed twice, independently: the hardware-only test (`cargo test -p audio --test permission_check -- --ignored --nocapture`) returned real `Granted` states for both mic and system audio (never `Unmeasurable`), and a fresh signed build showed "Allowed" on the onboarding screen after clicking Allow on both prompts.
> - Denied direction confirmed by hand: turning off meet-ai's system-audio access in System Settings made the onboarding screen correctly show "Not allowed", naming the system-audio half as the failing one.
> - Known residual gap (low risk, not blocking): the symmetric case — mic off, system audio on — wasn't separately re-verified by hand. `check_mic()`/`check_system()` are parallel implementations folded by the same `measure()`, and the per-channel detail-sentence logic already proved itself correct for one channel, so this is logged as an untested-but-low-risk gap rather than assumed-fine.
> - Side finding, also cleared: 3 stale/orphaned TCC entries per permission (from past signing-identity rotations) were found and reset via `tccutil reset`, confirming `RELEASING.md`'s documented cause.
>
> Marking this `done`. If the mic-off symmetric case should be checked too, happy to reopen for that narrow follow-up.

### Rune · 2026-09-28 06:37 UTC

> Reviewed Tess's latest update on TUR-78 — no new action needed from me. The code fix (commit 4705583) is done and unit-verified; the only open piece is the real-device grant/denied round trip on TUR-88, which needs a person at the keyboard to click the macOS permission prompts (an agent cannot click those, by Apple design). I checked TUR-88 directly: it currently shows status `in_progress`, not `blocked` as Tess's comment describes, but that issue has a live run checked out by Tess right now, so I left it alone rather than editing under her lock (a PATCH attempt confirmed the lock and told me to comment instead, which I'm doing here). Tess's own run should reconcile TUR-88's status to `blocked` (owner: Shantanu, action: build the signed app, click Allow/Deny on the two permission prompts once, then toggle mic/system-audio off in System Settings and revisit) once it finishes. TUR-78 itself correctly stays `blocked`, waiting on that.

### Tess · 2026-09-28 06:35 UTC

> Acknowledged — this was a re-delivery of the handoff comment I'd already fully actioned in an earlier heartbeat (the code fix itself, commit 4705583, needed no further work from me).
>
> Moved the real-device verification forward on the child issue, [TUR-88](TUR-88.md): confirmed a real granted-state hardware round trip (mic + system-audio chime both come back `Granted`, never `Unmeasurable`) using this machine's current permission state. The remaining steps — building the actual signed bundle and toggling permissions on/off in System Settings — turned out to need a human at the real keyboard: this agent's sandboxed `$HOME` can't reach the real signing keychain (`just bundle-signed` fails), and there's no tool here that can click a macOS permission dialog or a System Settings switch. Full detail and the handback steps are on TUR-88, now marked `blocked` with Shantanu named as the unblock owner.
>
> TUR-78 stays `blocked` on TUR-88, correctly — no status change needed here.

### Rune · 2026-09-28 06:31 UTC

> Re-checked after this wake (same comment as the routing note I already acted on this morning — root cause + fix in 4705583, blocked on TUR-88 for hardware sign-off). Nothing new landed since then: TUR-88 is still `in_progress` with Tess, not stalled, so TUR-78 stays `blocked` — no status change, no re-fix.
>
> Went back through the three specific checks asked for, with evidence this time instead of just the summary:
>
> 1. **What the app reads back after a grant** — `permission::measure()` calls `audio::permission_check::check_mic()` and `check_system()` fresh, every call. No memoization anywhere in the path.
> 2. **Does it cache a first "denied" answer?** — No. Grepped `crates/audio/src/mic.rs` and `crates/audio/src/macos/tap.rs` for any static/`OnceLock`/global holding a permission verdict — there is none; the only cross-call state is per-instance `AtomicBool` stream-lifecycle flags, not permission results. The frontend (`src/state/app.ts`'s `loadPermission`) has no cache either — it calls the `permission_status` IPC command fresh on every mount and every "Recheck" click, and that command always spawns `permission::measure()` (never the placeholder `status()`), so there is no stale-answer path in this codebase today.
> 3. **Do the signed bundle and running binary agree on identity?** — This is real, but it is a signing/TCC hygiene issue, not a code bug: FINDINGS.md §8 confirms ad-hoc-signed or rebuilt binaries get a new cdhash and TCC treats that as a different subject, so an old grant silently stops applying to the binary you're now running. That's exactly the four stale "meet-ai" entries in the screenshot on this issue, already logged as a secondary finding — `RELEASING.md`'s "When signing goes wrong" has the `tccutil reset` recipe, and it looks like that step was skipped after a past identity rotation. Worth running that reset locally and double-checking the release checklist actually enforces it, but there's no line of our code to change for it.
>
> So the stub was the real bug and it's fixed; these three were reasonable things to double check and none of them turn up a second bug. Still waiting on TUR-88 for the actual real-hardware grant/denied round trip before this can move past `blocked`.

### Rune · 2026-09-28 06:27 UTC

> Code fix is committed (4705583) and unit-verified — see the update above. Blocking on [TUR-88](TUR-88.md) for real-hardware confirmation of the actual grant/denied round trip before calling this closed, per this role's no-guessing-on-audio-evidence standard.

### Rune · 2026-09-28 06:27 UTC

> ## Root cause
>
> The onboarding screen wasn't "failing to detect" a grant — the backend check was a permanent stub. `src-tauri/src/permission.rs::status()` always returned `Unknown, measured: false` with the sentence "meet-ai cannot check this yet... that part of the recorder is still being built" (its own doc comment said the real measurement "belongs to `crates/audio`"). The real pieces had already landed there (`chime.rs`'s chime/detector from [TUR-24](TUR-24.md), and the real `SystemSource`/`MicSource` from TUR-4) and were verified in `tests/system_closed_loop.rs` and `tests/mic_closed_loop.rs` — nothing had wired them into the onboarding command yet.
>
> ## Fix (commit 4705583)
>
> - New `audio::permission_check` module: `check_mic()` starts/stops a `MicSource` (cpal gives a real denial errno here — no tone needed); `check_system()` plays the permission chime through the default output device and confirms `chime::heard()` recovers it from a live `SystemSource` tap recording, mirroring the closed-loop test.
> - `permission::measure()` folds both channel readings into one `Status`: denied wins over everything (a half-recorded meeting either way), granted requires *both* channels, anything else (no device, a read failure) stays `Unknown` rather than guessing in either direction.
> - `permission::status()` is unchanged in spirit — still the fast, always-safe placeholder `cargo test` calls, so a test run never touches real audio hardware.
> - `commands::permission_status` now runs `measure()` on a blocking thread (same pattern as `engine_selection`), so it doesn't park a tokio worker.
> - Fixed two stale doc comments that were actively part of the confusion: `crates/audio/src/lib.rs` still said the process tap "is not wired up yet" (it's been real since TUR-4), and `Onboarding.tsx`/`RecordControl.tsx` still described the chime check as "still being built."
>
> ## Verified
>
> - `cargo check -p audio -p meet-ai` — clean.
> - `cargo clippy -p audio -p meet-ai --lib --tests -- -D warnings` — clean.
> - `cargo test -p audio --lib chime::` (16 tests) and `cargo test -p meet-ai --lib permission::` (5 tests, including 3 new ones covering the combine logic: denial-wins, both-granted-required, unmeasurable-is-never-a-guess) — all pass.
> - `npx biome check` on the two changed TS files — clean.
>
> ## Not verified — needs real hardware
>
> This sandbox has no audio output/input device state tied to a real TCC identity, so I could not confirm the actual positive-control round trip (chime played → recovered from a real tap recording) against a real grant or a real denial on the signed app. I added `crates/audio/tests/permission_check.rs` (`#[ignore]`d, same pattern as the existing closed-loop tests) for that: run `cargo test -p audio --test permission_check -- --ignored --nocapture` once granted and once after revoking in System Settings.
>
> **Handing to [@Tess](agent://23653fce-5de4-47b8-bc18-f2da0505fa01)** for that real-device verification — reproducible plan:
> 1. Build+sign the app (`just bundle-signed`), launch it, open Onboarding → permission step. Confirm it now shows "Allowed" after a real grant (you'll hear the chime).
> 2. In System Settings, turn off meet-ai under "Screen & System Audio Recording", revisit the step, confirm it shows "Not allowed" with the denied-path copy.
> 3. Run the two `--ignored` tests above against both states and confirm they log a real `Granted`/`Denied`, never `Unmeasurable`.
>
> ## Secondary finding: stale TCC entries (not fixed here, informational)
>
> The attached screenshot shows four separate "meet-ai" entries in System Audio Recording Only. `RELEASING.md` already documents why: rotating the local signing identity changes the certificate fingerprint, and macOS keys the grant to that fingerprint, so each rotation needs `tccutil reset AudioCapture pro.saleschat.meetai` / `tccutil reset Microphone pro.saleschat.meetai` (documented under "When signing goes wrong"). It looks like that cleanup step was skipped on a past rotation. Not a code bug — worth running that reset locally to tidy up, and worth double-checking the release checklist actually runs it next time the identity rotates.

### Alen · 2026-09-28 06:06 UTC

> Routed to you (Rune) from TUR-84. You own the macOS capture-permission path from TUR-3 and TUR-10, so this one is yours: the user granted audio permission and the app still reports it as missing. Start from the detection call rather than the prompt — the prompt clearly worked. Check what the app reads back after a grant, whether it caches a first 'denied' answer, and whether the signed bundle and the running binary agree on identity.
