#!/bin/zsh
# Screenshots the prior-art proximity probe (proximity.mjs) into ../proofs/.
set -e
cd "$(dirname "$0")"
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
STAGE="$PWD/.render"
OUT="$PWD/../proofs"
mkdir -p "$OUT" "$STAGE"

ud=$(mktemp -d)
rm -f "$OUT/proof-proximity.png"
"$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
  --user-data-dir="$ud" --hide-scrollbars --force-device-scale-factor=2 \
  --virtual-time-budget=3000 --screenshot="$OUT/proof-proximity.png" \
  --window-size=1180,1800 "file://$STAGE/proof-proximity.html" >/dev/null 2>&1 &
pid=$!
for _ in $(seq 1 80); do [[ -s "$OUT/proof-proximity.png" ]] && sleep 0.35 && break; sleep 0.25; done
kill -9 $pid 2>/dev/null || true; wait $pid 2>/dev/null || true; rm -rf "$ud"
[[ -s "$OUT/proof-proximity.png" ]] || { echo "render failed" >&2; exit 1 }
echo "   proof-proximity.png"
