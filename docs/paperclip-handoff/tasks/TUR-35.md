# TUR-35 — Decide whether to ship an Icon Composer .icon (needs Xcode 26, not installed)

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | low |
| Owner | Alen |
| Created | 2026-09-27 13:54 UTC by Leo |
| Blocked because | Needs Alen: Waiting on the user to install Xcode 26 and comment here that xcrun --find actool resolves. On that comment, Alen moves TUR-87 to todo, which wakes Leo and starts the TUR-87 - TUR-85 - TUR-86 chain. |

## Sub-tasks

- [TUR-85](TUR-85.md) **backlog** — Compile the .icon into the Tauri bundle — actool, CFBundleIconName, re-sign
- [TUR-86](TUR-86.md) **blocked** — Verify the Icon Composer .icon on a real signed bundle
- [TUR-87](TUR-87.md) **backlog** — Author the meet-ai.icon in Icon Composer and wire it into render.sh

## Description

#### The decision

The meet-ai app icon is a legacy `.icns`. On macOS 26 that means we do not control the icon container — the system applies its own, and it applies a *different* one below 32pt than above it. [TUR-22](TUR-22.md) is the defect that came out of that; the fix landed there works, but it works by **giving up the small end entirely** and letting macOS synthesise 16px and 32px from the 128pt art.

Apple's supported answer is an **Icon Composer `.icon` asset**. It is the only way to control the container rather than have one applied to us, and it is the only way to keep per-size artwork on macOS 26.

#### Why this needs you and not me

**Icon Composer ships with Xcode 26. This workspace has Command Line Tools only.**

```
xcode-select -p   ->  /Library/Developer/CommandLineTools
xcrun --find actool  ->  not found
iconutil --help   ->  converts icns | iconset, nothing else
```

Installing Xcode is ~10GB and a toolchain change, which is outside what I should do on a design ticket. That is the call I need from you.

#### What it buys, concretely

1. **The 16px and 32px hand-drawn art becomes usable again.** Right now those drawings exist and are correct, and the app icon cannot use them — including them is what triggers the defect. With a `.icon` they are addressable per size.
2. **The macOS 14.4 floor stops being a trade.** SPEC L2 puts the floor at 14.4. Those releases do not re-render, so after the TUR-22 fix they downscale 16/32px from the 128pt rep instead of using art drawn for the size. I could not test that — this machine is Darwin 27 only. A `.icon` for 26 plus the legacy reps for older releases serves both without either compromise.
3. It is where the platform is going; a legacy `.icns` will keep drifting.

#### What happens if the answer is no

Nothing breaks. The TUR-22 fix stands on its own and the icon is correct on macOS 26 at every size I could measure. The residual is the untested 14.x/15.x small-size softness and the unused small art. This is an improvement ticket, not a blocker — I am filing it so the trade-off is recorded rather than quietly forgotten.

#### If the answer is yes

I build the `.icon`, wire it into `render.sh` alongside the `.icns`, and hand it to [@Tess](agent://23653fce-5de4-47b8-bc18-f2da0505fa01) to re-verify on a real signed bundle with the harness already in the repo at `design-system/meet-ai/brand/tools/iconprobe/`.

#### Related

- [TUR-22](TUR-22.md) — the defect and the landed fix
- [TUR-14](TUR-14.md) — Tess's signed-bundle verification, and my measurements


## Document: Icon Composer .icon — implementation plan

_Key `icon-composer-implementation-plan`, last updated 2026-09-28 12:29 UTC._

### Icon Composer `.icon` — implementation plan

**Decision: go.** Recorded 2026-09-28, re-confirmed by the user the same day after the fact-check
below. This document is the plan that follows from it, plus the facts I checked in the repo and on
Apple's side that change *why* we are doing it and *what it costs*.

**Revision 2 (2026-09-28)** closes the open question in 1.2 — the answer is no, `.icon` has no
per-size artwork — and adds 1.2b, which is good news: our macOS 26 floor means we skip the hybrid
fallback mess that makes this migration painful for everyone else.

---

#### 1. What changed since the ticket was written

The ticket argued three benefits. Two of them no longer hold as written, and one of my own objections
turned out to be wrong in our favour.

##### 1.1 The macOS 14.4 floor is gone — benefit #2 is dead

`SPEC.md:555` (**A8**, 2026-09-27) raised the OS floor to **macOS 26+** and closed TUR-37.
`src-tauri/tauri.conf.json:41` already reads `"minimumSystemVersion": "26.0"`, and `SPEC.md:203`
sets `LSMinimumSystemVersion = 26.0`.

There is no supported release below 26. So:

- "The macOS 14.4 floor stops being a trade" — there is no sub-26 tier left to trade against.
- The residual I named in my own earlier comment — *softer 16px/32px on macOS 14.x and 15.x* — does
  not exist either. Nobody we support takes that path.

##### 1.2 `.icon` is one layered composition, not per-size art — benefit #1 is dead (settled)

**Resolved 2026-09-28, from Apple's own documentation.** This was the open question in the previous
revision. It is now closed, and the answer is no.

Apple's *Creating your app icon using Icon Composer* states it directly:

> "The system automatically renders your app icon for the different platforms, appearances, and
> sizes from your single Icon Composer file."

One 1024×1024 canvas (1088 for watchOS), organised into at most four layer groups, with appearance
variants for light / dark / tinted / clear. There is **no per-pixel-size override anywhere in the
format.** `actool` does emit every size variant — 16, 32, … 512 — into `Assets.car`, but it
*generates* them from the single composition. It does not accept substitute artwork for a size.

So `design-system/meet-ai/brand/meet-ai-appicon-16-fullcolor.svg` **still never reaches the app
icon**, with or without a `.icon`. TUR-35's headline benefit does not exist.

That file remains exactly right where it is already used — the favicon and `.ico` outputs — and
those are untouched by this work.

##### 1.2b The macOS 26-only floor removes the ugliest part of this job

This is new, and it is in our favour.

The known hard problem with `.icon` adoption is shipping *both* a Liquid Glass icon for macOS 26 and
a correctly-rounded legacy icon for Sequoia and earlier. Developers hit it constantly: the
`ASSETCATALOG_OTHER_FLAGS = --enable-icon-stack-fallback-generation=disabled` workaround was
undocumented, worked in Xcode 26.0–26.0.1, and **stopped working in 26.1+**. The current advice is a
manual hybrid — build `Assets.car` under an old Xcode, generate the `.icns` separately, hand-merge
both plus `Info.plist` keys. (See the Apple Developer Forums thread in Sources.)

**None of that applies to us.** Per 1.1 our floor is macOS 26. We ship one icon path, for one OS
generation, with no fallback tier to reconcile. Any guide describing the hybrid dance is solving a
problem we do not have — and whoever picks this up should not copy that complexity in.

##### 1.3 Xcode is a one-time cost, not a build dependency — my "Cost 1" was wrong

I previously said this amends `SPEC.md:716` (A2: *"full Xcode is not required (Command Line Tools
suffice)"*). It does not, or not much.

`actool` compiles the `.icon` into a binary **`Assets.car`**, and that file is portable — it can be
committed and shipped from a Command-Line-Tools-only machine. Xcode 26 is needed to *regenerate* the
catalog when the icon art changes, not to build the app.

So the spec note becomes a narrow one: *"Xcode 26 is required only to regenerate
`src-tauri/icons/Assets.car` from the `.icon` source; routine builds still need only Command Line
Tools."* That is an addendum, not a reversal of A2.

##### 1.4 What the case now rests on

With 1.1 and 1.2 both now subtracted as *settled* facts, the remaining argument is the third
one — and it is a real one:

> On macOS 26 — the only OS we support — a legacy `.icns` means the system picks our icon container
> for us. A `.icon` is the supported way to control it, gets the Liquid Glass treatment right by
> construction instead of by the TUR-22 workaround, and stops the asset drifting further from the
> platform.

The current icon is correct on 26 at every size Leo measured. This is a durability-and-correctness
improvement, not a defect fix.

---

#### 2. The build recipe

Verified against Apple's `actool` interface and two independent write-ups of the non-Xcode
(Electron/Tauri-shaped) path.

##### 2.1 Compile

```bash
actool "$BRAND/meet-ai.icon" --compile "$OUT" \
  --output-format human-readable-text --notices --warnings --errors \
  --output-partial-info-plist "$OUT/icon-partial.plist" \
  --app-icon Icon --include-all-app-icons \
  --enable-on-demand-resources NO \
  --development-region en \
  --target-device mac \
  --minimum-deployment-target 26.0 \
  --platform macosx
```

Produces `Assets.car`. `--app-icon Icon` must match the `.icon` filename stem.

##### 2.2 Info.plist

```xml
<key>CFBundleIconName</key>
<string>Icon</string>
```

Keep `CFBundleIconFile` / the existing `.icns` **as well**. Both ship. The `.icns` is the fallback
path and costs nothing to keep.

##### 2.3 Tauri wiring — the part that is genuinely new

`src-tauri/tauri.conf.json:39` lists icons as a flat array ending in `icons/icon.icns`. The Tauri
bundler copies those files in; it has no `actool` step and produces no `Assets.car`. It also **signs
during bundling**.

So the pipeline needs, after `tauri build`:

1. Copy `Assets.car` into `<App>.app/Contents/Resources/`.
2. Merge `CFBundleIconName` into `Contents/Info.plist`.
3. **Re-sign the bundle** — steps 1 and 2 invalidate the signature Tauri just applied.

Step 3 is the real cost and the reason this needs an engineer, not just a designer.

##### 2.4 `render.sh`

`design-system/meet-ai/brand/tools/render.sh:82-127` builds the `.icns` and carries a long comment
block explaining the TUR-22 small-end decision. That comment stays accurate and should be **kept**,
with a pointer added to the `.icon` path rather than replaced. The `.icns`, `.ico` and favicon
outputs are all unchanged.

---

#### 3. The one step I cannot do

**Xcode 26 is not installed and I cannot install it.** Confirmed this run:

```
xcode-select -p        ->  /Library/Developer/CommandLineTools
xcrun --find actool    ->  error: unable to find utility "actool"
ls /Applications/Xcode*.app  ->  no matches
brew info --cask xcode ->  No Cask with this name exists
```

Disk is fine — 118 GB free. The obstacle is that every install route (Mac App Store, Apple Developer
Downloads, or the `xcodes` CLI) authenticates against an **Apple ID with two-factor**. That needs a
person.

Either of these works:

```bash
### Mac App Store (simplest)
open "macappstore://apps.apple.com/app/xcode/id497799835"

### or, to pin a specific 26.x
brew install xcodes && xcodes install --latest
```

Then, so the toolchain actually points at it:

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
xcrun --find actool   # should now resolve
```

---

#### 4. Work breakdown

| # | Owner | Deliverable |
|---|-------|-------------|
| 0 | **You** | Install Xcode 26, point `xcode-select` at it. Blocks everything below. |
| 1 | **Leo** | Author `meet-ai.icon` from the existing brand SVGs; wire it into `render.sh` alongside the `.icns`. 1.2 is settled — no per-size art, do not plan around it. |
| 2 | **Rune** | `actool` compile step, `CFBundleIconName`, post-bundle copy + **re-sign**, SPEC A2 addendum per 1.3. |
| 3 | **Tess** | Verify on a real signed bundle with `design-system/meet-ai/brand/tools/iconprobe/` — the same harness as TUR-14. Compare against the TUR-22 baseline at every rung. |

Each is a child issue of TUR-35, chained with real blockers so the next owner wakes automatically.

#### 5. What this is now worth — read before spending Rune's time

The 1.2 question is answered, so the honest accounting is:

- Benefit #1 (per-size hand-drawn art) — **does not exist.** The format has no such feature.
- Benefit #2 (the 14.4 floor) — **does not exist.** SPEC A8 removed the floor it traded against.
- Benefit #3 (control the icon container on macOS 26 instead of letting the system pick one) —
  **stands, and is the entire remaining case.**

That is a narrower prize than the ticket described, and the cost in 2.3 — post-bundle copy, plist
merge, re-sign — is unchanged. It is still worth doing: on the only OS we support, a legacy `.icns`
means the container is chosen for us, and that drift only grows.

But it is now a platform-alignment change, not a fix, and it should be scheduled like one. If Tess's
rungs in item 3 come back indistinguishable from today's `.icns` on macOS 26, that is the expected
result, not a failure — the win is in what stops going wrong later, not in a visible difference
today.

#### Sources

- [Supporting Liquid Glass Icons in Apps Without Xcode — Hendrik Erz](https://www.hendrik-erz.de/post/supporting-liquid-glass-icons-in-apps-without-xcode)
- [Create icons with Icon Composer — WWDC25 session 361](https://developer.apple.com/videos/play/wwdc2025/361/)
- [Updating application icons for macOS 26 Tahoe and Liquid Glass — Successful Software](https://successfulsoftware.net/2025/09/26/updating-application-icons-for-macos-26-tahoe-and-liquid-glass/)
- [Icon Composer: any way to add icons for older macOS versions? — Apple Developer Forums 794485](https://developer.apple.com/forums/thread/794485)
- [Creating your app icon using Icon Composer — Apple Developer](https://developer.apple.com/documentation/Xcode/creating-your-app-icon-using-icon-composer)
- [macOS packaging requires Xcode 26 since the switch to the Icon Composer icon — stoatchat/for-desktop#318](https://github.com/stoatchat/for-desktop/issues/318)


## Questions you answered

- **Q:** The app icon works correctly on macOS 26 today. Moving to an Icon Composer .icon would make the small-size artwork usable again on older macOS, but costs an Xcode 26 install, a spec amendment, and a new re-signing step in the build. Which way do you want to go?
  **A:** Yes — install Xcode 26 and build the .icon now
- **Q:** Since you said go: the macOS 14.4 floor is gone (SPEC A8 raised it to 26+), and a .icon most likely gives one layered design rather than per-size art — so the 16px hand-drawn file probably still won't reach the app icon. The cost is lower than I said, though: Xcode 26 is a one-time regeneration tool, not a build dependency. What's left is controlling the icon container on macOS 26 instead of lett…
  **A:** Still go — I'll install Xcode 26

## Comments (5)

### Alen · 2026-09-28 12:31 UTC

> ## Go confirmed. I settled the open question while you install — and the answer is no.
>
> Two things done this run, plus the one item that was waiting on a person.
>
> ### The per-size question is closed. There is no per-size artwork.
>
> This was the "ten minutes in Icon Composer, report back before building anything" task sitting at the top of TUR-87. It didn't need Icon Composer — Apple's own documentation answers it:
>
> > "The system automatically renders your app icon for the different platforms, appearances, and sizes from your single Icon Composer file."
> > — *Creating your app icon using Icon Composer*
>
> One 1024×1024 canvas, at most four layer groups, appearance variants for light / dark / tinted / clear. **No per-pixel-size override exists in the format.** `actool` does emit 16/32/…/512 into `Assets.car`, but it *generates* them from the single composition — it won't take substitute artwork for a size.
>
> So `meet-ai-appicon-16-fullcolor.svg` still never reaches the app icon, with or without a `.icon`. That was this ticket's headline benefit and it does not exist.
>
> **Where that leaves the case.** All three original arguments, honestly scored:
>
> | # | Argument | Status |
> |---|---|---|
> | 1 | 16/32px hand-drawn art becomes usable | **Dead** — the format has no such feature |
> | 2 | The macOS 14.4 floor stops being a trade | **Dead** — SPEC A8 removed the floor |
> | 3 | Control the icon container instead of the system picking one | **Stands. This is the whole case now.** |
>
> That third one is real: on macOS 26, the only OS we support, a legacy `.icns` means the container is chosen for us, and the drift only grows. But it's a platform-alignment change, not a fix, and it should be scheduled like one. **Your go still holds unless you want to revisit it** — I've built the chain on argument 3 alone and said so plainly on every ticket.
>
> ### Good news you didn't have before: we skip the ugly part
>
> The reason `.icon` migration has a bad reputation is shipping Liquid Glass for 26 *and* a properly-rounded legacy icon for Sequoia and earlier. The workaround for that (`--enable-icon-stack-fallback-generation=disabled`) was undocumented, worked in Xcode 26.0–26.0.1, and **broke in 26.1+**; the current advice is a manual hybrid involving building under an old Xcode. Developers are still stuck on it.
>
> **None of it applies to us** — our floor is macOS 26, so there's one icon path and nothing to reconcile. This is a meaningfully smaller job than the ecosystem chatter suggests.
>
> ### TUR-87 is assigned to Leo now
>
> The assignment went through on this run — it failed before, it worked this time, so that item is off your plate. The chain is complete and self-running: **TUR-87 (Leo) → TUR-85 (Rune) → TUR-86 (Tess)**, chained with real blockers, all in backlog.
>
> Also updated this run:
> - **Plan document revision 2** — section 1.2 rewritten with the settled answer, new 1.2b on the fallback question, section 5 replaced with the honest accounting above.
> - **TUR-87** — the blocking research question removed, replaced with the answer and a "don't design around per-size art" note. Saves Leo the detour.
> - **TUR-85** (Rune) — scope note: skip the hybrid recipe, and the prize is narrower than the ticket said.
> - **TUR-86** (Tess) — expectation set: rungs identical to today's `.icns` is the *success* case, not a null result. What matters is no regression, and that `codesign --verify` passes after the re-sign.
>
> ### Still waiting on you — one line
>
> ```bash
> sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
> sudo xcodebuild -license accept
> xcrun --find actool     # this resolving is the signal
> ```
>
> Verified again this run: `xcode-select -p` is still `/Library/Developer/CommandLineTools`, `actool` not found, no `Xcode.app`. Disk is fine — 103 GB free.
>
> Comment here when `actool` resolves and I'll move TUR-87 to todo, which wakes Leo and starts the chain. Marking this blocked on that.

### Alen · 2026-09-28 06:12 UTC

> Plan document and TUR-85/86/87 are up. Waiting on the saved question card: Xcode 26 needs your Apple ID, and two of the three reasons you approved this on did not survive fact-checking.

### Alen · 2026-09-28 06:11 UTC

> ## Go recorded. Plan and three child tickets are up — one step needs your hands.
>
> Plan document on this issue: **Icon Composer .icon — implementation plan**. Children:
>
> - **TUR-87** — author `meet-ai.icon`, wire it into `render.sh` *(unassigned, see below)*
> - **TUR-85** — Rune: `actool` compile, `CFBundleIconName`, post-bundle copy + re-sign
> - **TUR-86** — Tess: verify on a real signed bundle with the `iconprobe` harness
>
> Chained with real blockers, all in backlog until Xcode exists.
>
> ### Before you install: the ticket's reasons did not survive fact-checking
>
> You said go against a three-item benefit list. Two of those items are now wrong, and one of my own objections was wrong in your favour. None of this reverses the decision — but you should see it, because it arrived after you answered.
>
> **1. The macOS 14.4 floor is gone.** SPEC **A8** (`SPEC.md:555`, 2026-09-27) raised the floor to **macOS 26+** and closed TUR-37; `tauri.conf.json:41` already says `"minimumSystemVersion": "26.0"`. So the ticket's benefit #2 — "the 14.4 floor stops being a trade" — has nothing left to trade against. The residual *I* named when I recommended waiting — soft 16px on 14.x/15.x — does not exist either. My earlier recommendation was partly argued from a floor that had already moved.
>
> **2. A `.icon` probably does not give per-size art.** Icon Composer takes one layered composition and the system generates every size. If that holds, the hand-drawn `meet-ai-appicon-16-fullcolor.svg` **still never reaches the app icon** — and that was the ticket's headline benefit. I could not settle it from the docs, so it is the first thing TUR-87 does: ten minutes in the tool, report back, before anyone builds anything.
>
> **3. I was wrong that this makes Xcode a build dependency.** `actool` emits a portable `Assets.car` that can be committed and shipped from a Command-Line-Tools-only machine. So SPEC A2 survives — Xcode 26 is needed only to *regenerate* the catalog when the art changes. The cost is lower than I told you.
>
> **What's left of the case:** on macOS 26 — the only OS we support — a legacy `.icns` means the system picks our icon container for us. A `.icon` is the supported way to control it. That is a real reason, and it is now the only one.
>
> ### What I can't do
>
> Xcode 26 is not installed and I can't install it. Verified this run: `xcode-select -p` is still `/Library/Developer/CommandLineTools`, `xcrun --find actool` errors, no `Xcode.app`, and there is no Homebrew cask for Xcode. Disk is fine (118 GB free). Every route — App Store, Apple Developer Downloads, or the `xcodes` CLI — authenticates against an **Apple ID with two-factor**. That needs you.
>
> ```bash
> open "macappstore://apps.apple.com/app/xcode/id497799835"    # simplest
> # or: brew install xcodes && xcodes install --latest
>
> sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
> sudo xcodebuild -license accept
> xcrun --find actool     # should resolve
> ```
>
> Ping this issue when that last line resolves and I'll move TUR-87 to todo.
>
> ### One thing for you, not me
>
> **TUR-87 is unassigned.** Leo is the right owner, but Paperclip blocks me from assigning a child back to the agent who delegated this issue to me. Assign it to Leo directly and the chain runs itself.

### Paperclip (automatic) · 2026-09-28 06:03 UTC

> Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.

### Alen · 2026-09-27 13:56 UTC

> ## I checked Leo's facts, and found two costs the ticket does not name
>
> **The toolchain claim holds.** `xcode-select -p` is `/Library/Developer/CommandLineTools`, `xcrun --find actool` errors out, and there is no Xcode in `/Applications`. Disk is not the obstacle — 101 GB free, so a ~10 GB install fits.
>
> **Cost 1 — saying yes amends a recorded decision.** SPEC A2 (`SPEC.md:644`) states plainly: *"full Xcode is not required (Command Line Tools suffice, since L2 drops ScreenCaptureKit)"*. Installing Xcode 26 to build the icon makes full Xcode a build dependency for anyone who touches the brand assets. That is a spec amendment, not just a download.
>
> **Cost 2 — Tauri does not compile asset catalogs.** `src-tauri/tauri.conf.json:39` lists icons as a flat array ending in `icons/icon.icns`. The Tauri bundler copies that file into the bundle; it has no step that runs `actool` or produces an `Assets.car`. So an `.icon` is not a drop-in swap for the `.icns` — it needs a post-bundle step that compiles the asset, writes the matching `Info.plist` key, and then **re-signs**, because Tauri signs during bundling. That is new pipeline surface, and it is the part that would need Tess to re-verify, not just the artwork.
>
> **What is actually at stake if we say no.** Softer 16px and 32px rendering on macOS 14.x and 15.x, which nobody has seen, because this machine is macOS 27 only. Everything Leo could measure on 26 is correct. The unused small art (`meet-ai-appicon-16-fullcolor.svg`) stays on the shelf.
>
> ## My recommendation: not now, and set a trigger
>
> The defect is closed. What is left is a cosmetic residual on two OS versions we cannot test here, against a spec amendment plus a signing-pipeline change. That trade gets much better later — at the point we first prepare a real distributed, notarized build, the signing pipeline is already being reworked and the `actool` step is marginal rather than novel. Doing it now means paying the integration cost twice.
>
> The trigger I would set: **first notarized release build, or the first time someone runs meet-ai on a 14.x/15.x machine and says the small icon looks wrong.** Either one flips this to worth doing.
>
> Decision card below — it is your call, and I will run whichever way you pick.
