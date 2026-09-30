#!/bin/zsh
# =============================================================================
# Shared page screenshotter for the brand proof scripts.
#
# Factored out after review (TUR-12). render.sh had grown a retry loop for
# Chrome's startup race; render-proof.sh and render-proximity.sh (since deleted) had not, and
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

# shoot_page <html-path> <out-png> <w> <h> [min-ink] [scale] [bg]
#
# scale and bg are what used to justify render.sh keeping its own copy of this
# loop (TUR-32). They are parameters, not a second implementation:
#
#            scale   bg                  min-ink
#   proofs     2     (Chrome's opaque    0.04
#                     default)
#   icons      1     00000000            0.10   see render.sh for the
#                     (transparent)             calibration
shoot_page() {
  local html="$1" out="$2" w="$3" h="$4" ink="${5:-0.04}" scale="${6:-2}" bg="$7"
  local attempt ud pid
  local -a bgflag
  [[ -n "$bg" ]] && bgflag=(--default-background-color="$bg")

  [[ -f "$html" ]] || {
    print -u2 "   missing input: $html"
    print -u2 "   run its generator first (node proof.mjs, via render-proof.sh)."
    print -u2 "   refusing to invoke Chrome — it would screenshot its own error page."
    return 1
  }

  for attempt in 1 2 3 4 5; do
    ud=$(mktemp -d)
    rm -f "$out"
    # --use-mock-keychain, or Chrome blocks on a GUI keychain prompt (TUR-20).
    # On startup Chrome's OSCrypt wants to store its "Chrome Safe Storage" key
    # in the default keychain. The --user-data-dir above is a fresh mktemp
    # profile, so there is never an existing key to reuse and every single
    # launch attempts that write. Under a redirected $HOME — which is what any
    # sandboxed runner gives us — there is no keychain at all to write to, so
    # the Security framework puts up a modal "A keychain cannot be found to
    # store Chrome" panel and waits. Headless does not suppress it. Chrome sits
    # on the dialog, writes nothing, the loop below times out, and the retry
    # raises another one: five stacked dialogs per image.
    "$CHROME" --headless --disable-gpu --no-first-run --no-default-browser-check \
      --use-mock-keychain \
      --user-data-dir="$ud" --hide-scrollbars --force-device-scale-factor="$scale" \
      --virtual-time-budget=3000 "${bgflag[@]}" --screenshot="$out" \
      --window-size="$w,$h" "file://$html" >/dev/null 2>&1 &
    pid=$!
    for _ in $(seq 1 80); do [[ -s "$out" ]] && sleep 0.35 && break; sleep 0.25; done
    kill -9 $pid 2>/dev/null || true
    wait $pid 2>/dev/null || true
    rm -rf "$ud"

    # `|| rc=$?` rather than a bare call followed by `$?`: every caller runs
    # under `set -e`, and a bare non-zero exit there aborts the whole script
    # before the next line can read the status — which made the retry loop
    # below unreachable and turned every transient Chrome flake into a hard
    # build failure. Putting the call on the left of `||` makes it a tested
    # condition, which ERR_EXIT ignores.
    local rc=0
    python3 "$SHOOT_DIR/verify_render.py" "$out" \
      --expect "$((w * scale))x$((h * scale))" --min-ink "$ink" || rc=$?
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
