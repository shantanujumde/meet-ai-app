#!/bin/zsh
# =============================================================================
# meet-ai brand — raster pipeline
# Run `node build.mjs` first. This turns the SVG masters into every raster the
# app ships: src-tauri/icons/*, public/* favicons, and the review proofs.
#
# Rasterising goes through headless Chrome because this machine has no
# rsvg-convert / ImageMagick / Inkscape. Chrome is also what actually renders
# the app's web view, so the SVG is measured by the same engine that ships it.
# =============================================================================
set -e
cd "$(dirname "$0")"

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
STAGE="$PWD/.render"
BRAND="$(cd .. && pwd)"
REPO="$(cd ../../../.. && pwd)"

[[ -d "$STAGE" ]] || { echo "run: node build.mjs" >&2; exit 1 }

# Chrome headless writes the screenshot and then hangs on shutdown in this
# environment, so it is killed once the file lands.
#
# It also loses the startup race intermittently here and exits without writing
# anything — this is not size-dependent (observed failing at 24 and 32 while 16
# and 64 succeeded in the same sweep), so a failure is retried rather than
# treated as fatal. The result is verified by pixel dimensions, not just by
# file existence, so a truncated write cannot pass.
shoot() { # shoot <url> <out> <w> <h>
  # Declared once: in zsh a repeated `local` in a loop body echoes the value.
  local attempt ud pid
  for attempt in 1 2 3 4 5; do
    ud=$(mktemp -d)
    rm -f "$2"
    "$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
      --user-data-dir="$ud" --hide-scrollbars --force-device-scale-factor=1 \
      --virtual-time-budget=3000 --default-background-color=00000000 \
      --screenshot="$2" --window-size="$3,$4" "$1" >/dev/null 2>&1 &
    pid=$!
    for _ in $(seq 1 80); do [[ -s "$2" ]] && sleep 0.35 && break; sleep 0.25; done
    kill -9 $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    rm -rf "$ud"
    if [[ -s "$2" ]] && [[ "$(sips -g pixelWidth "$2" 2>/dev/null | tail -1 | tr -dc 0-9)" == "$3" ]]; then
      return 0
    fi
    print -u2 "   retry $attempt: ${2:t}"
    sleep 0.6
  done
  echo "render failed after 5 attempts: $2" >&2; exit 1
}

echo "-- rasterising"
# Only the raster targets build.mjs emitted. proof.mjs also writes .html into
# this directory and those are page screenshots, not icon rasters.
for html in "$STAGE"/icon-*.html "$STAGE"/mark-*.html; do
  name="${html:t:r}"
  size="${name##*-}"
  shoot "file://$html" "$STAGE/$name.png" "$size" "$size"
  printf '   %-12s %s\n' "$name" "$(sips -g pixelWidth -g pixelHeight "$STAGE/$name.png" | tr -d '\n' | sed 's/.*pixelWidth: //;s/  pixelHeight: /x/')"
done

echo "-- src-tauri/icons"
ICONS="$REPO/src-tauri/icons"
mkdir -p "$ICONS"
cp "$STAGE/icon-32.png"   "$ICONS/32x32.png"
cp "$STAGE/icon-64.png"   "$ICONS/64x64.png"
cp "$STAGE/icon-128.png"  "$ICONS/128x128.png"
cp "$STAGE/icon-256.png"  "$ICONS/128x128@2x.png"
cp "$STAGE/icon-512.png"  "$ICONS/icon.png"

# .icns — a real multi-resolution bundle via iconutil. The @2x entries use the
# art drawn for the *point* size, not the pixel size: icon_16x16@2x is a 32px
# raster of the 16pt design.
SET="$STAGE/meet-ai.iconset"
rm -rf "$SET"; mkdir -p "$SET"
cp "$STAGE/icon-16.png"   "$SET/icon_16x16.png"
cp "$STAGE/icon-32.png"   "$SET/icon_16x16@2x.png"
cp "$STAGE/icon-32.png"   "$SET/icon_32x32.png"
cp "$STAGE/icon-64.png"   "$SET/icon_32x32@2x.png"
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

echo "-- public/ favicons"
PUB="$REPO/public"
mkdir -p "$PUB"
cp "$BRAND/meet-ai-favicon.svg" "$PUB/favicon.svg"
cp "$STAGE/icon-16.png"  "$PUB/favicon-16.png"
cp "$STAGE/icon-32.png"  "$PUB/favicon-32.png"
cp "$STAGE/icon-180.png" "$PUB/apple-touch-icon.png" 2>/dev/null || \
  cp "$STAGE/icon-256.png" "$PUB/apple-touch-icon.png"

echo "-- done"
