#!/bin/zsh
# Screenshots the prior-art proximity probe into ../proofs/.
#
# The generator runs here rather than being a step you have to remember: the
# probe stages its HTML into .render/, which build.mjs also writes to, so an
# intervening rebuild used to delete the input. Owning the input removes the
# ordering hazard instead of documenting it.
set -e
cd "$(dirname "$0")"
source ./_shoot.zsh

STAGE="$PWD/.render"
OUT="$PWD/../proofs"
mkdir -p "$OUT"

echo "-- proximity probe"
node proximity.mjs
# 1900, not 1800: at 1800 the final row (app icon at actual size) had its
# labels cut off by the bottom edge.
shoot_page "$STAGE/proof-proximity.html" "$OUT/proof-proximity.png" 1180 1900
