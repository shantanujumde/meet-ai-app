#!/bin/zsh
# =============================================================================
# meet-ai brand — raster pipeline
# Run `node build.mjs` first. This turns the SVG masters into every raster the
# app ships: src-tauri/icons/*, public/* favicons, and the review proofs. It
# also writes ../meet-ai.icon, the Icon Composer source for Assets.car.
#
# Rasterising goes through headless Chrome because this machine has no
# rsvg-convert / ImageMagick / Inkscape. Chrome is also what actually renders
# the app's web view, so the SVG is measured by the same engine that ships it.
# =============================================================================
set -e
cd "$(dirname "$0")"
# One screenshot loop for the whole brand pipeline (TUR-32). This script used
# to carry its own inline copy, and being a copy it never picked up either
# guard added in 64e2a4e — so the files that actually ship were the only ones
# rendered without a missing-input check and without a content check. Its sole
# verification was pixel dimensions, which TUR-12 had already proved cannot see
# a Chrome error page: Chrome renders ERR_FILE_NOT_FOUND at exactly the window
# size you asked for. The drift was live, not theoretical — the TUR-20
# --use-mock-keychain fix had to be written here a second time, comment and
# all, one commit after the copies were meant to be consolidated.
source ./_shoot.zsh

STAGE="$PWD/.render"
BRAND="$(cd .. && pwd)"
REPO="$(cd ../../../.. && pwd)"

[[ -d "$STAGE" ]] || { echo "run: node build.mjs" >&2; exit 1 }

# Icon rasters differ from the proof pages in exactly two arguments: they are
# shot at 1x (the raster *is* the deliverable, so its pixel size is the size
# asked for) onto a transparent field rather than Chrome's opaque default.
ICON_SCALE=1
ICON_BG=00000000

# Ink floor for the icon path. The 4% default is calibrated for dense proof
# pages and is too low here for one reason and too high for another, so it was
# re-derived from the real files rather than reused:
#
#   real icon-path rasters   28% (mark-128, mark-512, the sparsest art)
#                            .. 85% (icon-16)
#   tray template            40% (tray-32), 46% (tray-16)
#   blank / broken-<img>     0%
#   ERR_FILE_NOT_FOUND       0% at 16px, 3% at 512px
#
# 3% is uncomfortably close to the 4% proof floor, so the icon path gets its
# own: 10% is 3.3x above the worst observed failure and 2.8x below the
# sparsest correct raster. Note the tray template only measures at all because
# verify_render.py now counts alpha as a channel — it is pure black artwork
# whose only varying channel is alpha, and on RGB alone it read 0% ink.
ICON_INK=0.10

echo "-- rasterising"
# Only the raster targets build.mjs emitted. proof.mjs also writes .html into
# this directory and those are page screenshots, not icon rasters.
for html in "$STAGE"/icon-*.html "$STAGE"/mark-*.html "$STAGE"/tray-*.html; do
  name="${html:t:r}"
  size="${name##*-}"
  shoot_page "$html" "$STAGE/$name.png" "$size" "$size" \
    "$ICON_INK" "$ICON_SCALE" "$ICON_BG"
done

echo "-- src-tauri/icons"
ICONS="$REPO/src-tauri/icons"
mkdir -p "$ICONS"
cp "$STAGE/icon-32.png"   "$ICONS/32x32.png"
cp "$STAGE/icon-64.png"   "$ICONS/64x64.png"
cp "$STAGE/icon-128.png"  "$ICONS/128x128.png"
cp "$STAGE/icon-256.png"  "$ICONS/128x128@2x.png"
cp "$STAGE/icon-512.png"  "$ICONS/icon.png"

# Menu-bar tray icon. The `…Template` / `…Template@2x` filenames are AppKit's
# own convention for a template image — black-with-alpha artwork that macOS
# tints itself, dark on a light menu bar and light on a dark one. Naming them
# this way keeps the intent attached to the files; src-tauri/src/tray.rs also
# says `icon_as_template(true)` so the flag does not depend on how the bytes
# get loaded. A plain, unflagged PNG here renders solid black on a dark menu
# bar, which looks like a bug rather than a choice.
cp "$STAGE/tray-16.png" "$ICONS/meet-aiTemplate.png"
cp "$STAGE/tray-32.png" "$ICONS/meet-aiTemplate@2x.png"

# .icns — a real multi-resolution bundle via iconutil. The @2x entries use the
# art drawn for the *point* size, not the pixel size: icon_128x128@2x is a 256px
# raster of the 128pt design.
#
# The 16pt and 32pt entries are deliberately NOT included (TUR-22).
#
# On macOS 26 a legacy `.icns` gets two different treatments depending on
# whether the requested size is backed by a real rep:
#
#   * rep present at that point size -> the old compositor. Our tile is shrunk
#     and pasted onto the system's light icon plate, giving a square inside a
#     square. At 16px the inner tile is ~10px, the brackets collapse, and the
#     whole thing inverts to a light frame around a dark smudge.
#   * no rep at that point size -> the modern container. The art is scaled to
#     fill and the system's own squircle mask and shadow are applied. Correct.
#
# Measured on a real signed bundle across six ladders. Shipping only 16pt reps
# breaks 16px and leaves 32px clean; shipping only 32pt reps breaks both, since
# 32pt is also what 16px falls back to. Shipping neither is clean everywhere,
# so the small end is left for macOS to synthesise from the 128pt art.
#
# This is why the 16px hand-drawn art no longer reaches the app icon. It is
# still the right drawing for the favicon and the .ico below, where nothing
# re-renders it and a pixel-grid drawing is exactly what is wanted.
#
# This used to cost something below macOS 26, where there is no such re-render
# and the system downscales 16px and 32px from the 128pt rep instead of using
# art drawn for the size. That cost is now zero: SPEC A8 raised the OS floor to
# macOS 26+, so no supported release takes that path. The measurement that
# settled it is TUR-41 -- brand/proofs/leo-tur41-downscale-proxy-16-32-64.png, a
# simulated shrink showing the mark stays readable but goes visibly soft at
# 16px. Kept here because it is the reason the ladder stops at 128pt.
#
# Still open, and unrelated to the floor: the proper fix is an Icon Composer
# `.icon` asset, which needs Xcode 26; only Command Line Tools are installed,
# so it cannot be produced in this workspace.
#
# Update (TUR-87): the `.icon` now exists, at ../meet-ai.icon, written further
# down by build-icon.mjs. Writing it needs only node; Xcode is needed only to
# compile it into Assets.car. It ships alongside this `.icns`, not instead of
# it, so everything above still describes the `.icns` exactly.
SET="$STAGE/meet-ai.iconset"
rm -rf "$SET"; mkdir -p "$SET"
cp "$STAGE/icon-128.png"  "$SET/icon_128x128.png"
cp "$STAGE/icon-256.png"  "$SET/icon_128x128@2x.png"
cp "$STAGE/icon-256.png"  "$SET/icon_256x256.png"
cp "$STAGE/icon-512.png"  "$SET/icon_256x256@2x.png"
cp "$STAGE/icon-512.png"  "$SET/icon_512x512.png"
cp "$STAGE/icon-1024.png" "$SET/icon_512x512@2x.png"
iconutil -c icns "$SET" -o "$ICONS/icon.icns"
echo "   icon.icns  $(du -h "$ICONS/icon.icns" | cut -f1)"

python3 make_ico.py "$ICONS/icon.ico" \
  16:"$STAGE/icon-16.png"   24:"$STAGE/icon-24.png"  32:"$STAGE/icon-32.png" \
  48:"$STAGE/icon-48.png"   64:"$STAGE/icon-64.png"  128:"$STAGE/icon-128.png" \
  256:"$STAGE/icon-256.png"

# Icon Composer document (TUR-87) — the macOS 26 source for Assets.car. Built
# from geometry.mjs, not from the rasters above, so it can never pick up a
# pre-masked squircle: the system draws the mask itself. See build-icon.mjs for
# the layer and appearance decisions.
echo "-- meet-ai.icon"
node build-icon.mjs "$BRAND/meet-ai.icon"

# ictool is Icon Composer's own renderer and the one thing that can say whether
# it accepts the document: a malformed icon.json fails it with a non-zero exit.
# It lives inside Xcode, so on a Command-Line-Tools-only machine this is
# skipped, not failed — the document above is still written either way.
ICTOOL="/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool"
if [[ -x "$ICTOOL" ]]; then
  "$ICTOOL" "$BRAND/meet-ai.icon" --export-image \
    --output-file "$STAGE/meet-ai-icon-preview.png" --platform macOS \
    --rendition Default --width 1024 --height 1024 --scale 1 >/dev/null
  echo "   ictool accepted it -> .render/meet-ai-icon-preview.png"
else
  echo "   ictool not found (needs Xcode); document written, not previewed"
fi

echo "-- public/ favicons"
PUB="$REPO/public"
mkdir -p "$PUB"
cp "$BRAND/meet-ai-favicon.svg" "$PUB/favicon.svg"
cp "$STAGE/icon-16.png"  "$PUB/favicon-16.png"
cp "$STAGE/icon-32.png"  "$PUB/favicon-32.png"
cp "$STAGE/icon-180.png" "$PUB/apple-touch-icon.png" 2>/dev/null || \
  cp "$STAGE/icon-256.png" "$PUB/apple-touch-icon.png"

echo "-- done"
