#!/bin/zsh
# Screenshots the review proofs into ../proofs/.
#
# Run after `node build.mjs && ./render.sh`: the proofs show the rasters those
# two produce, and ictool renders of the .icon they write, rather than a fresh
# drawing of the geometry.
#
# The generator runs here, not as a step you have to remember: its pages are
# staged into .render/ alongside the rasters build.mjs owns, and a rebuild
# between generate and screenshot would delete them.
set -e
cd "$(dirname "$0")"
source ./_shoot.zsh

STAGE="$PWD/.render"
OUT="$PWD/../proofs"
BRAND="$(cd .. && pwd)"
mkdir -p "$OUT"

[[ -f "$STAGE/icon-256.png" && -f "$STAGE/tray-32.png" ]] || {
  echo "no rasters in .render/ — run: node build.mjs && ./render.sh" >&2
  exit 1
}

# What macOS 26 actually draws is the .icon, so the icon proof shows ictool's
# renders of it next to the legacy rasters. <pt>@<scale>x, rendered the way the
# system asks for them. Without Xcode the section is left out and the page
# says so; the rest of the proofs do not need it.
XCODE_DEVELOPER_DIR="${XCODE_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
ICTOOL="${XCODE_DEVELOPER_DIR%/Developer}/Applications/Icon Composer.app/Contents/Executables/ictool"
rm -f "$STAGE"/app-*.png(N)
if [[ -x "$ICTOOL" ]]; then
  echo "-- meet-ai.icon renders (ictool)"
  for r in Default Dark TintedLight TintedDark ClearLight ClearDark; do
    for spec in 16@1 32@1 16@2 32@2 64@2 128@2 256@2; do
      pt=${spec%@*} sc=${spec#*@}
      "$ICTOOL" "$BRAND/meet-ai.icon" --export-image --output-file "$STAGE/app-$r-$pt@${sc}x.png" \
        --platform macOS --rendition $r --width $pt --height $pt --scale $sc >/dev/null
    done
  done
else
  echo "-- ictool not found (needs Xcode): the .icon section is left out of proof-app-icon.png"
fi

echo "-- proofs"
node proof.mjs

shoot_page "$STAGE/proof-icon.html"    "$OUT/proof-app-icon.png"   1180 2240
shoot_page "$STAGE/proof-menubar.html" "$OUT/proof-menu-bar.png"    900  800
shoot_page "$STAGE/proof-logo.html"    "$OUT/proof-logo-system.png" 820 1400
shoot_page "$STAGE/proof-readme.html"  "$OUT/proof-in-context.png" 1010  660
shoot_page "$STAGE/proof-misuse.html"  "$OUT/proof-misuse.png"     1000  880
