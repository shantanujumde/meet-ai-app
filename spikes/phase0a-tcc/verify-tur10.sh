#!/usr/bin/env bash
# TUR-10 — the two things that need a human at the keyboard.
#
# Everything else in TUR-10 was measured without one (FINDINGS.md §10). Two
# facts could not be: TCC only writes a grant *record* when someone clicks
# Allow, and an explicit "Don't Allow" is the one denial an automated run cannot
# produce. This script wraps both so the human part is two clicks; it does the
# resetting, rebuilding, measuring and writing-up itself.
#
#   ./verify-tur10.sh
#
# Takes ~2 minutes. Writes /tmp/meet-ai-tur10/report.md. Plays a tone.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$HERE/build/meet-ai.app"
BUNDLE_ID="pro.saleschat.meetai"
IDENTITY="${SIGN_IDENTITY:-meet-ai Local Signing}"
KEYCHAIN="${SIGN_KEYCHAIN:-$HOME/Library/Keychains/meet-ai-signing.keychain-db}"
ROOT=/tmp/meet-ai-tur10
REPORT="$ROOT/report.md"
SECS=12

rm -rf "$ROOT"; mkdir -p "$ROOT"

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
ask() { printf '\n\033[1;33m>>> %s\033[0m\n' "$*"; }

if ! security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "no signing identity '$IDENTITY' — run ./make-identity.sh first" >&2
  exit 1
fi

sign_build() { SIGN_IDENTITY="$IDENTITY" SIGN_KEYCHAIN="$KEYCHAIN" "$HERE/build.sh" >"$ROOT/build.log" 2>&1; }

# Run the bundle once. $1 = output dir, $2 = extra probe args.
run_once() {
  local out="$1"; shift
  rm -rf "$out"; mkdir -p "$out"
  date +%s > "$out/.start"
  # `open`, not a direct exec: LaunchServices makes the *app* the TCC
  # responsible process. Launching the helper from a shell attributes the
  # prompt to the terminal and invalidates the result.
  open -a "$APP" --args --out "$out" --seconds "$SECS" "$@"
  for _ in $(seq 1 $(( SECS + 120 ))); do
    [[ -f "$out/app-done.json" ]] && break
    sleep 1
  done
}

# Everything tccd said about us since a run started.
tcc_since() {
  local out="$1"
  local elapsed=$(( $(date +%s) - $(cat "$out/.start") + 5 ))
  /usr/bin/log show --last "${elapsed}s" --style compact \
      --predicate 'subsystem == "com.apple.TCC"' 2>/dev/null \
    | grep -iE "meetai" || true
}

# Pull the numbers that matter out of probe-result.json, plus an ffmpeg-free
# but completely literal "is any byte non-zero" check on the WAV payload.
summarise() {
  local out="$1"
  python3 - "$out" <<'PY'
import json, sys, pathlib
out = pathlib.Path(sys.argv[1])
p = out / "probe-result.json"
if not p.exists():
    print("| (no probe-result.json — the probe never finished) | |"); sys.exit()
t = json.load(p.open())["system_tap"]; m = t["measured"]
rows = [
    ("create_tap_osstatus",  t["create_tap_osstatus"]),
    ("create_ioproc_ms",     round(t["timings_ms"]["create_ioproc_ms"], 3)),
    ("device_start_ms",      round(t["timings_ms"]["device_start_ms"], 3)),
    ("frames",               m["frames"]),
    ("io_callbacks",         m["io_callbacks"]),
    ("rms",                  m["rms"]),
    ("peak",                 m["peak"]),
    ("zero_sample_fraction", m["zero_sample_fraction"]),
]
wav = out / "system.wav"
if wav.exists():
    payload = wav.read_bytes()[44:]
    rows.append(("non-zero payload bytes", sum(1 for b in payload if b)))
for k, v in rows:
    print(f"| `{k}` | {v} |")
PY
}

{
  echo "# TUR-10 — human-in-the-loop verification"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) on macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion)), $(uname -m)."
  echo
  echo '```'
  codesign -d -r- "$APP" 2>&1 | grep designated || true
  echo '```'
} > "$REPORT"

say "building, signed with '$IDENTITY'"
sign_build

# ---------------------------------------------------------------------------
say "1/3  fresh grant — you will be asked to click ALLOW"
# ---------------------------------------------------------------------------
tccutil reset AudioCapture "$BUNDLE_ID" || true
tccutil reset Microphone   "$BUNDLE_ID" || true
ask "A dialog will appear. Click  ALLOW."
run_once "$ROOT/granted"
tcc_since "$ROOT/granted" > "$ROOT/granted.tcc.log"

GRANT_EVENT=$(grep -o "Publishing <TCCDEvent:[^>]*kTCCServiceAudioCapture[^>]*>" "$ROOT/granted.tcc.log" | head -1 || true)
{
  echo; echo "## 1. Grant creation"
  echo; echo "The question: does TCC store this by bundle ID now, rather than by path?"
  echo; echo '```'
  echo "${GRANT_EVENT:-(no TCCDEvent Create line — nothing was granted)}"
  grep -o "AUTHREQ_SUBJECT: msgID=[0-9.]*, subject=[^,]*" "$ROOT/granted.tcc.log" | sed 's/msgID=[0-9.]*, //' | sort -u | head -3
  echo '```'
  echo
  if [[ "$GRANT_EVENT" == *"identifier_type=Bundle ID"* ]]; then
    echo "**Bundle ID keyed.** FINDINGS §10.4 confirmed; SPEC §6's \`tccutil reset\` recipe is valid."
  elif [[ -n "$GRANT_EVENT" ]]; then
    echo "⚠️ **Not bundle-ID keyed** — FINDINGS §10.4 is wrong and SPEC §6 needs another fix."
  else
    echo "⚠️ No grant was created. Was the dialog answered?"
  fi
  echo; echo "| measure | granted |"; echo "|---|---|"
  summarise "$ROOT/granted"
} >> "$REPORT"

# ---------------------------------------------------------------------------
say "2/3  rebuild + rerun, unattended — does the grant survive a new cdhash?"
# ---------------------------------------------------------------------------
printf '\n// tur10 rebuild marker %s\n' "$(date +%s)" >> "$HERE/src/app/main.swift"
trap 'git -C "$HERE" checkout -- src/app/main.swift 2>/dev/null || true' EXIT
sign_build
run_once "$ROOT/rebuilt"
tcc_since "$ROOT/rebuilt" > "$ROOT/rebuilt.tcc.log"

{
  echo; echo "## 2. Grant survival across a rebuild"
  echo; echo "Same bundle ID, same certificate, different binary. Under ad-hoc signing this"
  echo "was where the grant intermittently vanished (FINDINGS §8, signing caveat)."
  echo; echo '```'
  grep -E "AUTHREQ_PROMPTING|Failed to match existing code requirement" "$ROOT/rebuilt.tcc.log" | head -3 \
    || echo "(no prompt, no code-requirement mismatch)"
  echo '```'
  echo
  if grep -q "AUTHREQ_PROMPTING" "$ROOT/rebuilt.tcc.log"; then
    echo "⚠️ **Re-prompted.** The grant did not survive the rebuild."
  else
    echo "**Survived.** No prompt, no mismatch — the cdhash-free designated requirement holds."
  fi
  echo; echo "| measure | after rebuild |"; echo "|---|---|"
  summarise "$ROOT/rebuilt"
} >> "$REPORT"

# ---------------------------------------------------------------------------
say "3/3  explicit denial — you will be asked to click DON'T ALLOW"
# ---------------------------------------------------------------------------
tccutil reset AudioCapture "$BUNDLE_ID" || true
ask "A dialog will appear. Click  DON'T ALLOW."
run_once "$ROOT/denied"
tcc_since "$ROOT/denied" > "$ROOT/denied.tcc.log"

DENY_RESULT=$(grep -A1 "AUTHREQ_PROMPTING.*kTCCServiceAudioCapture" "$ROOT/denied.tcc.log" | grep -o "authValue=[0-9]*, authReason=[0-9]*" | head -1 || true)
{
  echo; echo "## 3. Explicit user denial"
  echo; echo "FINDINGS §10.1 measured a denial with \`authReason=8\` (missing usage string) and"
  echo "found the API returns \`noErr\` and delivers bit-exact zeros. A user pressing"
  echo "**Don't Allow** is \`authReason=2\`. This step checks they behave the same."
  echo; echo '```'
  echo "${DENY_RESULT:-(no AUTHREQ_RESULT captured)}"
  echo '```'
  echo; echo "| measure | denied |"; echo "|---|---|"
  summarise "$ROOT/denied"
  echo
  echo "Expected, if §10.1 generalises: \`create_tap_osstatus\` = 0, \`create_ioproc_ms\` a"
  echo "few ms with no error, callbacks still firing, \`zero_sample_fraction\` = 1.0, and"
  echo "zero non-zero payload bytes — i.e. the API reports success and records silence."
} >> "$REPORT"

tccutil reset AudioCapture "$BUNDLE_ID" || true

say "done — $REPORT"
cat "$REPORT"
