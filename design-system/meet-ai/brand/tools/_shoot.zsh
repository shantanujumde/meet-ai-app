#!/bin/zsh
# =============================================================================
# Shared page screenshotter for the brand proof scripts.
#
# Factored out after review (TUR-12). render.sh had grown a retry loop for
# Chrome's startup race; render-proof.sh and render-proximity.sh had not, and
# the one that had drifted furthest is the one that shipped a broken proof.
# One implementation, so a fix lands everywhere.
#
# Two guards, because they catch different failures:
#
#   1. The input HTML must exist *before* Chrome is invoked. Chrome does not
#      fail on a missing file:// URL — it renders its own ERR_FILE_NOT_FOUND
#      page and screenshots that, at exactly the window size requested. A
#      non-empty check passes. So does a pixel-width check. Only looking at
#      the input first, or at the content after, can see it.
#
#   2. The captured PNG must actually have content (verify_render.py). This
#      covers the rest of the class: a page that loads but whose CSS or <img>
#      rasters are missing, or a blank render.
# =============================================================================
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
SHOOT_DIR="${0:A:h}"

# shoot_page <html-path> <out-png> <w> <h> [min-ink]
shoot_page() {
  local html="$1" out="$2" w="$3" h="$4" ink="${5:-0.04}"
  local attempt ud pid

  [[ -f "$html" ]] || {
    print -u2 "   missing input: $html"
    print -u2 "   run its generator first (node proof.mjs / proximity.mjs / concepts.mjs)."
    print -u2 "   refusing to invoke Chrome — it would screenshot its own error page."
    return 1
  }

  for attempt in 1 2 3 4 5; do
    ud=$(mktemp -d)
    rm -f "$out"
    "$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
      --user-data-dir="$ud" --hide-scrollbars --force-device-scale-factor=2 \
      --virtual-time-budget=3000 --screenshot="$out" \
      --window-size="$w,$h" "file://$html" >/dev/null 2>&1 &
    pid=$!
    for _ in $(seq 1 80); do [[ -s "$out" ]] && sleep 0.35 && break; sleep 0.25; done
    kill -9 $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    rm -rf "$ud"

    python3 "$SHOOT_DIR/verify_render.py" "$out" \
      --expect "$((w * 2))x$((h * 2))" --min-ink "$ink"
    local rc=$?
    (( rc == 0 )) && return 0
    # 3 = the page rendered at the right size but is empty. Deterministic, so
    # another attempt would only produce the same empty picture.
    (( rc == 3 )) && { print -u2 "   not a transient failure — not retrying"; return 1 }
    print -u2 "   retry $attempt: ${out:t}"
    sleep 0.6
  done

  print -u2 "render failed after 5 attempts: $out"
  return 1
}
