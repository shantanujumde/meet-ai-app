#!/bin/zsh
# Screenshots the review proofs into ../proofs/.
#
# Both generators run here for the same reason render-proximity.sh runs its
# own: these pages are staged into .render/ alongside the rasters build.mjs
# owns, and a rebuild between generate and screenshot would delete them.
set -e
cd "$(dirname "$0")"
source ./_shoot.zsh

STAGE="$PWD/.render"
OUT="$PWD/../proofs"
mkdir -p "$OUT"

echo "-- proofs"
node proof.mjs
node concepts.mjs

shoot_page "$STAGE/proof-icon.html"    "$OUT/proof-app-icon.png"   1180 1760
shoot_page "$STAGE/proof-menubar.html" "$OUT/proof-menu-bar.png"    900  760
shoot_page "$STAGE/proof-logo.html"    "$OUT/proof-logo-system.png" 760 1180
shoot_page "$STAGE/proof-readme.html"  "$OUT/proof-in-context.png" 1010  680
shoot_page "$STAGE/proof-misuse.html"  "$OUT/proof-misuse.png"     1000  780
shoot_page "$PWD/../concepts/_directions.html" "$OUT/proof-directions.png" 1080 1080
