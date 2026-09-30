#!/bin/zsh
# realprobe.sh <label> <icns>
# Takes the REAL Tauri-built, signed meet-ai.app, swaps in a candidate .icns,
# re-signs with the real identity, gives it a unique bundle id and a normal
# ~/Applications location, registers it with LaunchServices, then renders what
# macOS hands back.
#
# The throwaway-bundle harness this replaces was not trustworthy: apps living
# under /private/var/folders fell back to the generic document icon partway
# through a session, so a "defect" could just be LaunchServices giving up. Every
# render here is therefore checked against a captured generic-icon baseline and
# the run fails rather than reporting a fallback as a result.
set -euo pipefail
# Paths come from where this script lives, not a fixed checkout: a hard-coded
# repo path wrote lab bundles into whichever checkout it named, even when run
# from a worktree (TUR-86). ICONPROBE_SRC points it at a different bundle.
REPO="${0:A:h:h:h:h:h:h}"
LAB="${ICONPROBE_OUT:-${0:A:h}}"
SRC="${ICONPROBE_SRC:-$REPO/target/release/bundle/macos/meet-ai.app}"
SYSICON="${SYSICON:-$REPO/target/icon-lab/bin/sysicon}"
[[ -x "$SYSICON" ]] || SYSICON="$LAB/sysicon"
LSREG="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
label="$1"; icns="$2"
stamp="$(date +%s)$RANDOM"
dest="$REPO/target/icon-lab"
app="$dest/meet-ai-$label.app"

mkdir -p "$dest"
rm -rf "$app"
ditto "$SRC" "$app"
cp "$icns" "$app/Contents/Resources/icon.icns"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier pro.saleschat.meetai.lab$stamp" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName meet-ai-$label" "$app/Contents/Info.plist" 2>/dev/null || true

# Re-sign: the icns is a sealed resource, so an unsigned swap invalidates it.
HOME=/Users/shantanujumde codesign --force --options runtime --timestamp=none \
  --keychain "/Users/shantanujumde/Library/Keychains/meet-ai-signing.keychain-db" \
  -s "meet-ai Local Signing" "$app" >/dev/null 2>&1
codesign --verify --strict "$app" || { echo "SIGN FAILED $label" >&2; exit 1 }

"$LSREG" -f "$app" >/dev/null 2>&1 || true
touch "$app"

mkdir -p "$LAB/out"
for px in 16 32 41 64 128; do
  "$SYSICON" "$app" "$px" "$LAB/out/R-$label-$px.png"
done
echo "$label signed+rendered: $app"
