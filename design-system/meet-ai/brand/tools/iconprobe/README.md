# iconprobe — what macOS actually draws for a bundle

Renders the icon macOS hands back for an `.app`, at exact pixel sizes, through
`NSWorkspace.icon(forFile:)` — the same path Finder and the Dock use. Built for
TUR-22, where the shipped `.icns` was correct on disk and wrong on screen.

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

## Use

```sh
swiftc -O sysicon.swift -o sysicon
swiftc -O sheet.swift   -o sheet

./probe-bundle.sh mylabel /path/to/candidate.icns     # needs a built bundle
./sheet out.png 12 "16=out/R-mylabel-16.png" "32=out/R-mylabel-32.png"
```

`sheet` magnifies nearest-neighbour and shows each render on dark and on light.
Nearest-neighbour is not cosmetic: any smoothing hides exactly the pixel-level
collapse these probes exist to catch.

`sysicon <app> <px> <out.png> [--reps]` — `--reps` prints the representation
sizes the system offers. Sizes that are not in your `.icns` (18x18, 24x24) mean
macOS is synthesising its own ladder rather than reading yours.

Clean up with `rm -rf target/icon-lab` when you are done.
