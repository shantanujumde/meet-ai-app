# iconprobe — what macOS actually draws for a bundle

Renders the icon macOS hands back for an `.app`, at exact pixel sizes, through
`NSWorkspace.icon(forFile:)` — the same path Finder and the Dock use. Built for
TUR-22, where the shipped `.icns` was correct on disk and wrong on screen, and
extended for TUR-86, the Icon Composer `.icon` check.

## Why it has to be a real bundle

The first version of this built throwaway `.app` stubs in a temp directory. It
was not trustworthy: LaunchServices stopped recognising apps under
`/private/var/folders` partway through a session and quietly fell back to the
generic document icon, so a "defect" could just be LaunchServices giving up.

`probe-bundle.sh` therefore starts from the real Tauri-built bundle, swaps in
the candidate `.icns`, re-signs it (the `.icns` is a sealed resource, so an
unsigned swap invalidates the signature), gives it a unique bundle id, and
leaves it under `target/icon-lab/` — a normal path LaunchServices will index.
Look at every render. A render that comes back as a blank page with a folded
corner is the generic fallback, not your icon.

**Pass `sysicon` an absolute path** — or use the current `sysicon`, which makes
it absolute for you. Given a relative path, `icon(forFile:)` does not fail; it
returns that same folded-page icon, which is indistinguishable from
LaunchServices giving up (found on TUR-86).

## Verifying a bundle end to end — `verify-icon.sh`

```sh
./verify-icon.sh /path/to/meet-ai.app --label baseline   # once, on a known-good bundle
./verify-icon.sh /path/to/meet-ai.app --label icon       # the bundle under test
./verify-icon.sh /System/Applications/Utilities/Terminal.app --label calib-terminal --against none
```

It builds the tools into `target/icon-lab/bin/` (gitignored — binaries are not
committed), never modifies the bundle you pass, and writes everything to
`target/icon-lab/runs/<label>/`: `summary.md`, `renders/`, `sheets/`, and each
trial's lab copy. It checks, in order:

1. `codesign --verify --deep --strict --verbose=2` on the bundle as given.
2. `Info.plist` has `CFBundleIconFile` (and the file exists) **and**
   `CFBundleIconName`.
3. `Contents/Resources/Assets.car` exists, and `assetutil --info` lists an app
   icon under the `CFBundleIconName` — with `IconImageStack` renditions, which
   is what a compiled Icon Composer `.icon` produces (Terminal.app's catalog has
   the same shape).
4. Every rung — 16 32 41 64 128 256 512 1024 at 1x, 16 32 128 256 512 at 2x —
   rendered from a re-signed lab copy with its own bundle id, each checked
   against the generic document and generic app icons.
5. **Which icon the system draws.** `--reps` cannot tell you: on macOS 26/27 it
   lists the same synthesised ladder for an `.icns`-only app and for
   Terminal.app's `Assets.car`. So the script swaps the `.icns` for a decoy
   (magenta, lime cross, same rep ladder) in one lab copy, and in a second copy
   removes `Assets.car` and `CFBundleIconName` as well. If the first renders
   unchanged and the second shows the decoy, the system is drawing
   `Assets.car`. A third copy with only `Assets.car` removed is the `.icns`
   fallback, compared against the baseline.
6. Rung by rung against `runs/baseline` (or `--against DIR`), with contact
   sheets pairing the two. Differences are marked REVIEW, not failed —
   better or worse is for a person looking at the sheets. A rung that cannot
   be compared at all (a missing baseline render, a size mismatch) is REVIEW
   too, with the reason, rather than ending the run.

Exit status is non-zero if any hard check fails. On an `.icns`-only bundle
(such as the pre-TUR-85 build recorded as `runs/baseline`) the three
`Assets.car` checks and "draws Assets.car" fail — that is the expected, clean
result for a bundle without the catalog. The current bundle ships
`Assets.car`, and passes them.

## The pieces

```sh
swiftc -O sysicon.swift -o sysicon
swiftc -O sheet.swift   -o sheet

./probe-bundle.sh mylabel /path/to/candidate.icns     # needs a built bundle
./sheet out.png 12 "16=out/R-mylabel-16.png" "32=out/R-mylabel-32.png"
```

`probe-bundle.sh` finds the repo from its own location; `ICONPROBE_SRC` points
it at a bundle other than `target/release/bundle/macos/meet-ai.app`.

`sheet` magnifies nearest-neighbour and shows each render on dark and on light.
Nearest-neighbour is not cosmetic: any smoothing hides exactly the pixel-level
collapse these probes exist to catch.

`sysicon <app> <pt> <out.png> [--scale N] [--reps]` — without `--scale` the
output is `<pt>` pixels square, as it always was. `--scale 2` draws the same
point size on a 2x backing store (the rep a Retina display uses). `--reps`
prints the representation sizes the system offers. Sizes that are not in your
`.icns` (18x18, 24x24) mean macOS is synthesising its own ladder rather than
reading yours. There is no dark-mode switch: drawing under a dark appearance
returns identical pixels, because the icon's dark/tinted/clear variant follows
the user's system icon style, not the drawing appearance.

`pixdiff <a.png> <b.png>` — mean and worst channel difference and the share of
pixels that moved. Two draws of the same icon measure `changed=0.00%`.

`decoy <out.png> [px]` — the decoy art `verify-icon.sh` builds its `.icns` from.

Clean up with `rm -rf target/icon-lab` when you are done.
