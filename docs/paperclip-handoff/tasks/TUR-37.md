# TUR-37 — Check the icon on the macOS 14.4 floor — the TUR-22 fix trades the small end away

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 14:02 UTC by Alen |
| Completed | 2026-09-27 14:31 UTC |
| Parent | [TUR-22](TUR-22.md) Icon is illegible at 16px and double-framed at 32px on macOS 26 |

## Sub-tasks

- [TUR-41](TUR-41.md) **done** — Approximate the pre-macOS-26 downscale at 16px and 32px — no 14.x host needed

## Description

#### Why this exists

[TUR-22](TUR-22.md) is fixed by **dropping the 16pt and 32pt entries from `icon.icns`**. Leo measured that on a real signed bundle: any rep at or below 32pt sends macOS 26 down the old compositor path, which plates our tile inside the system's icon container and inverts it. With those reps gone, macOS 26 synthesises 16px and 32px from the 128pt art and composes them correctly.

**That fix is correct for macOS 26 and untested below it.** Leo said so explicitly rather than papering over it, and the check has no home ticket, so this is it.

#### The specific risk

Older macOS does **not** re-render legacy `.icns`. It uses the reps it is given. We now give it none at 16pt or 32pt, so on those releases the OS will downscale from the 128pt rep instead of drawing the art that was hand-tuned for 16px.

Expected outcome: **correctly composed but soft** — no double-framing, no inversion, just a blurrier mark in Finder list view, column view, the sidebar and Open/Save panels. That is the hypothesis. It is not measured, and "expected soft" could equally turn out to be "illegible smudge", which is the same user-visible failure TUR-22 was opened for.

SPEC L2 puts our floor at **macOS 14.4**, so this is inside what we claim to support.

#### Why nobody has run it

Every machine in this workspace is Darwin 27 (macOS 26). There is no 14.x or 15.x host to test on. This is a hardware gap, not a skill or time gap.

#### Unblock owner and action

**Owner: the board / user.** One of these two, whichever is cheaper:

1. **Supply a macOS 14.x or 15.x machine or VM** with the signed bundle on it. Then the check is about ten minutes: drop `meet-ai.app` in `/Applications`, look at Finder list view and column view, and compare against the macOS 26 renders in `design-system/meet-ai/brand/proofs/leo-tur22-E-fixed-ladder-16-to-128.png`.
2. **Accept the risk and raise the floor**, i.e. decide meet-ai ships macOS 26+ and amend SPEC L2. This is not free — it cuts the addressable machines — but it makes the question moot.

If the answer is (2), close this as `done` with the SPEC edit rather than leaving it open.

#### How to run it, once a machine exists

The harness is already in the repo and needs no Xcode — `design-system/meet-ai/brand/tools/iconprobe/` (`README.md`, `sysicon.swift`, `sheet.swift`, `probe-bundle.sh`), built with `swiftc`.

```bash
sysicon /Applications/meet-ai.app 16 out16.png
sysicon /Applications/meet-ai.app 32 out32.png
```

Two cautions from Leo, both learned the hard way:

- **Do not probe a throwaway `.app` in a temp directory.** LaunchServices stops recognising bundles under `/private/var/folders` partway through a session and silently returns the generic blank-document icon, so a "defect" can just be LaunchServices giving up. Probe a real, signed bundle and confirm each render is not the generic fallback.
- **Re-render, do not reuse old proofs.** The art has already changed under two people on this ticket chain.

#### Not in scope

Whether to ship an Icon Composer `.icon` instead — that is [TUR-35](TUR-35.md) and it is waiting on a call from the board. If we go that way, per-size artwork comes back and this ticket's risk disappears with it.

## Commits that mention this task

- `27aba18` 2026-09-27 — TUR-37: raise the OS floor to macOS 26+ (SPEC A8)

## Work products

- artifact: leo-tur41-downscale-proxy-16-32-64.png

## Comments (5)

### Alen · 2026-09-27 14:30 UTC

> Re-checked on reassignment: the work on this ticket is already in the tree and unchanged. Commit `27aba18` is present on `chore/claude-setup-and-design-system`, `SPEC.md` L2 reads **macOS 26+** with amendment **A8**, and `src-tauri/tauri.conf.json` reads `minimumSystemVersion: "26.0"`.
>
> The status field had stayed at `in_progress` even though the last run closed the work out. Setting it to `done` now. Nothing further is needed here — the board decision on [TUR-40](TUR-40.md) removed the macOS 14.4 target this check was aimed at.

### Alen · 2026-09-27 14:30 UTC

> ## Resolved by decision, not by measurement — the board raised the OS floor
>
> The board answered the question on [TUR-40](TUR-40.md): **meet-ai ships macOS 26 and later only.** That closes this ticket. There is no longer a macOS 14.4 floor to check the icon against, so the check this issue asks for has no supported target.
>
> ### What I changed in the tree
>
> Commit `27aba18`, six files:
>
> - **`SPEC.md` L2** — floor `macOS 14.4+` → **`macOS 26+`**, with a new amendment **A8** recording the decision, the evidence behind it, what it changes, and what it deliberately does *not* decide.
> - **`src-tauri/tauri.conf.json`** — `minimumSystemVersion` `14.4` → **`26.0`**. This is the real build config, not just the spec.
> - **`SPEC.md` §2.4/§2.5** — engine defaults are now `mac → Apple, always`. `whisper-rs` on mac becomes a **manual override**, never a version fallback. It stays the Windows engine and stays the safety net behind the preview WinRT APIs.
> - **`SETUP.md`, `CONTRIBUTING.md`** — preflight checks now say `>= 26.0`.
> - **`crates/stt/src/whisper.rs`** — the doc comment no longer claims it serves macOS 14.4–25.
> - **`design-system/meet-ai/brand/tools/render.sh`** — the comment stating the 14.4 trade-off as an unpaid cost now records that the cost is zero, and points at the TUR-41 proof.
>
> ### What I left alone on purpose
>
> `FINDINGS.md` and `spikes/phase0a-tcc/build.sh` still say 14.4. They are records of what was true when that research and that Phase 0a spike ran. Rewriting them would falsify a past result rather than update a decision. A8 says so explicitly, so nobody reads them later as drift.
>
> ### Verified, not assumed
>
> - `tauri.conf.json` still parses and reads back `26.0`.
> - `cargo fmt -p stt --check` clean, `cargo check -p stt` clean.
> - `biome check` clean on the changed JSON.
> - `zsh -n render.sh` clean (it is a zsh script — my first parse check used bash and misreported).
> - Grep for `14.4` across the repo returns only the two files named above and A8's own description of the change.
>
> ### One thing this does *not* spend
>
> Whether mac should keep a whisper path **at all** is a separate scope question. Dropping it would also delete the model download manager, the Metal build config and mac-side VAD chunking from v1 — real savings, but it removes the mac safety net if Apple's engine disappoints. A8 records it as open. Raise it as its own ticket if you want it decided.
>
> Closing this `done`.

### Alen · 2026-09-27 14:20 UTC

> The trigger I named above has fired: [TUR-41](TUR-41.md) is done, and the board decision this ticket waits on is now raised as a question card on [TUR-40](TUR-40.md).
>
> Three options on the card: accept Leo's simulated shrink as good enough and keep macOS 14/15 support, ship macOS 26+ only and amend SPEC L2, or supply a 14.x/15.x machine and run the real probe that is already written.
>
> I verified the evidence before raising it — I read the comparison sheet myself, and confirmed `src-tauri/icons/icon.icns` was not touched by TUR-41's commit. My own read of the 16px cell: readable, but clearly softer than the old hand-drawn rep. The board should decide knowing both halves of that.
>
> This ticket stays `blocked` until the card is answered. I carry the answer back here and close it out.

### Leo · 2026-09-27 14:17 UTC

> Cross-posting the result of TUR-41 (the proxy check I ran to make this decision informed without a 14.x/15.x host):
>
> **Verdict: soft but readable at 16px, 32px and 64px — no collapse, no illegible smudge.** The icon downscaled from the shipped 128pt art (via a Core Graphics high-quality resample, standing in for how legacy macOS would render it) is softer and greyer than the old hand-tuned rep at 16px, but the bracket-and-dot mark still reads clearly. At 32px and 64px it's close to indistinguishable from the old hand-tuned rep. Full comparison sheet attached (same file as on TUR-41).
>
> **Caveat, in plain terms:** this is a stand-in, not the real macOS 14.4 test. It can't match the OS's exact resize filter or any sharpening it applies, and it can't test the separate double-frame bug from TUR-22 (that one comes from how the newer OS composes the icon, not from resizing a flat image). So this is good evidence the "illegible smudge" fear is unlikely — not proof.
>
> Given that, my read: the risk this ticket is blocked on looks low. If the board wants certainty rather than strong evidence before deciding between buying a 14.x/15.x machine and narrowing the supported OS range, this still needs a real 14.x/15.x host — full details and the reasoning are on TUR-41.

### Alen · 2026-09-27 14:13 UTC

> Watchdog note from [TUR-40](TUR-40.md).
>
> This ticket has a real blocker (no macOS 14.x/15.x host or VM on this machine — confirmed, host is macOS 27.0 build 26A428, no UTM/Parallels/VMware/VirtualBox installed) but its structured unblock-owner field is empty, so it does not surface anywhere a human will look.
>
> I tried to set it to `board`. The platform refuses: **agents may only name themselves as an unblock owner** (HTTP 403). That is a rule, not a glitch — a board-owned question has to go through a decision card raised by whoever holds the issue, not through an agent writing the owner field.
>
> I am deliberately **not** raising that card yet. [TUR-41](TUR-41.md) (Leo, in progress) is producing a same-machine proxy of the pre-macOS-26 downscale at 16px and 32px. Asking the board to choose between buying a 14.x machine and cutting the supported OS range is a coin flip until that picture exists. Once Leo posts the verdict on TUR-41 and mirrors it here, the question becomes answerable and the card should go up then.
>
> So: this stays `blocked`, owner is the **board** in substance even though the field is empty, and the trigger to ask is TUR-41 landing.
