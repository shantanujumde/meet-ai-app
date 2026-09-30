#!/usr/bin/env bash
# TUR-10 — the two things that need a human at the keyboard.
#
# Everything else in TUR-10 was measured without one (docs/findings.md §10). Two
# facts could not be: TCC only writes a grant *record* when someone clicks
# Allow, and an explicit "Don't Allow" is the one denial an automated run cannot
# produce. This script wraps both so the human part is two clicks; it does the
# resetting, rebuilding, measuring and writing-up itself.
#
#   scripts/signing/verify-tur10.sh
#
# Takes ~2 minutes. Writes /tmp/meet-ai-tur10/report.md. Plays a tone.
set -euo pipefail

# The spike files this drives (build.sh, auto-click.sh, src/) stay in spikes/phase0a-tcc.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../spikes/phase0a-tcc" && pwd)"
APP="$HERE/build/meet-ai.app"
BUNDLE_ID="pro.saleschat.meetai"
IDENTITY="${SIGN_IDENTITY:-meet-ai Local Signing}"
REAL_HOME="$(/usr/bin/dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
[[ -n "$REAL_HOME" && -d "$REAL_HOME" ]] || REAL_HOME="$HOME"
KEYCHAIN="${SIGN_KEYCHAIN:-$REAL_HOME/Library/Keychains/meet-ai-signing.keychain-db}"
ROOT=/tmp/meet-ai-tur10
REPORT="$ROOT/report.md"
SECS=12

rm -rf "$ROOT"; mkdir -p "$ROOT"

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
ask() { printf '\n\033[1;33m>>> %s\033[0m\n' "$*"; }

# The two consent clicks. With AUTO_CLICK=1 an Accessibility-driven clicker
# answers them instead of a person — see auto-click.sh for the guards that stop
# it touching anything but our own prompt. Falls back to asking when
# Accessibility is not granted, because a silent no-op here would look exactly
# like "the dialog never appeared" in the report.
AUTO_CLICK="${AUTO_CLICK:-0}"
if [[ "$AUTO_CLICK" == 1 ]] && ! osascript -e 'tell application "System Events" to return count of every process' >/dev/null 2>&1; then
  echo "AUTO_CLICK=1 but Accessibility is not granted to this process — falling back to manual clicks." >&2
  AUTO_CLICK=0
fi

CLICKER_PID=""
# Arm the clicker BEFORE the prompt is triggered; it polls for the dialog.
arm_click() {
  local mode="$1"
  [[ "$AUTO_CLICK" == 1 ]] || { ask "A dialog will appear. Click  ${2}."; return; }
  say "auto-answering the prompt with '${2}'"
  "$HERE/auto-click.sh" "$mode" meet-ai 120 >"$ROOT/click-$mode.log" 2>&1 &
  CLICKER_PID=$!
}
# Collect the clicker's verdict. A timeout is a hard failure: every measurement
# after an unanswered prompt describes the wrong thing.
reap_click() {
  [[ -n "$CLICKER_PID" ]] || return 0
  if wait "$CLICKER_PID"; then
    echo "    $(cat "$ROOT/click-$1.log")"
  else
    echo "!! the auto-clicker never found our prompt (see $ROOT/click-$1.log)." >&2
    echo "!! Re-run without AUTO_CLICK=1 and click by hand, or widen" >&2
    echo "!! AUTO_CLICK_PROCS if the dialog is drawn by another process." >&2
    exit 1
  fi
  CLICKER_PID=""
}

# Pass the keychain explicitly. A bare `find-identity` reads the user's keychain
# search list, which is empty under a sandboxed $HOME — it then reports "0 valid
# identities" for an identity that is present and working.
if ! security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null | grep -qF "$IDENTITY"; then
  echo "no signing identity '$IDENTITY' in $KEYCHAIN — run scripts/signing/make-identity.sh first" >&2
  exit 1
fi

# The leaf SHA-1 is what TCC keys every grant to. If it has moved since the last
# run, every result below is meaningless: a re-prompt could be a correct re-ask
# after denial, or just macOS not recognising a differently-signed binary. Bail
# rather than hand back an ambiguous gate result.
LEAF="$(security find-certificate -c "$IDENTITY" -p "$KEYCHAIN" 2>/dev/null \
        | openssl x509 -outform der 2>/dev/null | shasum -a 1 | awk '{print $1}')"
if [[ -z "$LEAF" ]]; then
  echo "could not read the leaf fingerprint from $KEYCHAIN" >&2
  exit 1
fi

# The known-good baseline, recorded when the identity was created (TUR-10).
# Override only if the identity was deliberately rotated.
EXPECT_LEAF="${EXPECT_LEAF:-eafb73d29b2f35ca25c2f9fd193869fd880a7e0d}"
if [[ "$LEAF" != "$EXPECT_LEAF" ]]; then
  echo "!! signing identity has ROTATED." >&2
  echo "!!   expected leaf: $EXPECT_LEAF" >&2
  echo "!!   actual leaf:   $LEAF" >&2
  echo "!! Every prior TCC grant is keyed to the old leaf, so a re-prompt here" >&2
  echo "!! would be unattributable. Re-run with EXPECT_LEAF=$LEAF once you have" >&2
  echo "!! accepted that every existing grant is void." >&2
  exit 1
fi

# Snapshot every TCC record that mentions us, for both services. `tccutil list`
# is a read verb and needs no privileges. Path-keyed records left over from the
# ad-hoc era show up here and CANNOT be cleared by `tccutil reset` — it resolves
# its argument through LaunchServices as a bundle ID and returns -10814 for a
# path. They are inert for an identity-signed build (tccd matches us by bundle
# ID) but they are why this snapshot is in the report: if a result looks wrong,
# check whether a stale path record shadowed it.
tcc_records() {
  echo "AudioCapture:"; tccutil list -s kTCCServiceAudioCapture 2>/dev/null | grep -i "meet" | sed 's/^/  /' || echo "  (none)"
  echo "Microphone:";   tccutil list -s kTCCServiceMicrophone   2>/dev/null | grep -i "meet" | sed 's/^/  /' || echo "  (none)"
}

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

# Everything tccd said since a run started — UNFILTERED on purpose.
#
# The obvious `| grep meetai` loses the answer. tccd splits a consent into two
# lines: AUTHREQ_PROMPTING carries the subject (our bundle id) and a msgID,
# and the AUTHREQ_RESULT that records what the user actually clicked carries
# only that msgID. Grepping for our name keeps the question and discards the
# answer, which is why an earlier run of this script reported "(no
# AUTHREQ_RESULT captured)" for a denial it had in fact captured. Correlation
# by msgID happens in decision_for(); it needs the whole log.
tcc_since() {
  local out="$1"
  local elapsed=$(( $(date +%s) - $(cat "$out/.start") + 5 ))
  /usr/bin/log show --last "${elapsed}s" --style compact \
      --predicate 'subsystem == "com.apple.TCC"' 2>/dev/null || true
}

# Only the lines that name us — for the human-readable excerpts.
ours() { grep -iE "meetai" "$1" || true; }

# The verdict tccd recorded for our AudioCapture prompt, resolved through the
# msgID. Prints e.g. "authValue=0, authReason=2" plus how long the prompt stood.
#
# authValue: 0 = denied, 1 = undetermined (the pre-prompt query), 2 = allowed.
# authReason 2 = "user answered the prompt" — it appears on BOTH an Allow and a
# Don't Allow, so authValue is what distinguishes them, not authReason.
decision_for() {
  python3 - "$1" <<'PY'
import re, sys
lines = open(sys.argv[1], errors='replace').read().splitlines()
prompt = None
for l in lines:
    m = re.search(r'AUTHREQ_PROMPTING: msgID=([0-9.]+), service=kTCCServiceAudioCapture.*meetai', l)
    if m:
        prompt = (m.group(1), l.split()[1][:12])
if not prompt:
    print("(no AUTHREQ_PROMPTING for kTCCServiceAudioCapture — no dialog was shown)")
    sys.exit()
msgid, t0 = prompt
for l in lines:
    m = re.search(r'AUTHREQ_RESULT: msgID=' + re.escape(msgid) + r', (authValue=\d+, authReason=\d+)', l)
    if m:
        t1 = l.split()[1][:12]
        def ms(t):
            h, mi, s = t.split(':'); return (int(h)*3600 + int(mi)*60 + float(s)) * 1000
        print(f"AUTHREQ_PROMPTING msgID={msgid} at {t0}")
        print(f"AUTHREQ_RESULT    msgID={msgid} at {t1} -> {m.group(1)}")
        print(f"dialog stood open for {ms(t1) - ms(t0):.0f} ms")
        sys.exit()
print(f"(prompt msgID={msgid} shown at {t0}, but no matching AUTHREQ_RESULT — unanswered?)")
PY
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
  echo "Signing identity \`$IDENTITY\`, leaf \`$LEAF\` — matches the recorded baseline."
  echo
  echo '```'
  codesign -d -r- "$APP" 2>&1 | grep designated || true
  echo '```'
  echo
  echo "## 0. TCC baseline before anything was touched"
  echo
  echo '```'
  tcc_records
  echo '```'
} > "$REPORT"

say "building, signed with '$IDENTITY' (leaf $LEAF)"
sign_build

# The build must still carry the leaf we just asserted. Signing can succeed
# against a different identity if the search list changed underneath us.
if ! codesign -d -r- "$APP" 2>&1 | grep -qi "$LEAF"; then
  echo "!! the built bundle is not signed by the expected leaf $LEAF" >&2
  codesign -d -r- "$APP" 2>&1 | grep designated >&2 || true
  exit 1
fi

# ---------------------------------------------------------------------------
say "1/3  fresh grant — you will be asked to click ALLOW"
# ---------------------------------------------------------------------------
# Reset BOTH services. They are independently tracked and were observed in
# different states (AudioCapture granted, Microphone not), which would otherwise
# mean only one prompt appears and the run looks half-broken.
# Note the exit codes: `tccutil reset` exits 64 with -10814 when LaunchServices
# cannot resolve the bundle ID. That is the signal the app is unknown to LS, not
# a harmless no-op, so record it rather than swallowing it.
for svc in AudioCapture Microphone; do
  if tccutil reset "$svc" "$BUNDLE_ID" >/dev/null 2>&1; then
    echo "    reset $svc -> ok"
  else
    echo "    reset $svc -> FAILED (LaunchServices does not know $BUNDLE_ID)"
  fi
done
arm_click allow "ALLOW"
run_once "$ROOT/granted"
reap_click allow
tcc_since "$ROOT/granted" > "$ROOT/granted.tcc.log"

# Specifically the Create. A reset emits a Delete for the same service first,
# and `head -1` picked that up — a Delete says nothing about how the new grant
# is keyed, which is the entire question this section asks.
GRANT_EVENT=$(grep -o "Publishing <TCCDEvent: type=Create[^>]*kTCCServiceAudioCapture[^>]*>" "$ROOT/granted.tcc.log" | head -1 || true)
{
  echo; echo "## 1. Grant creation"
  echo; echo "The question: does TCC store this by bundle ID now, rather than by path?"
  echo; echo '```'
  echo "${GRANT_EVENT:-(no TCCDEvent Create line — nothing was granted)}"
  ours "$ROOT/granted.tcc.log" | grep -o "AUTHREQ_SUBJECT: msgID=[0-9.]*, subject=[^,]*" | sed 's/msgID=[0-9.]*, //' | sort -u | head -3
  decision_for "$ROOT/granted.tcc.log"
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
  # Scoped to lines naming us: the log is unfiltered now, and any other app
  # prompting for anything during these 12 seconds would otherwise be read as
  # our grant having evaporated.
  ours "$ROOT/rebuilt.tcc.log" | grep -E "AUTHREQ_PROMPTING|Failed to match existing code requirement" | head -3 \
    || echo "(no prompt, no code-requirement mismatch)"
  echo '```'
  echo
  if ours "$ROOT/rebuilt.tcc.log" | grep -q "AUTHREQ_PROMPTING"; then
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
arm_click deny "DON'T ALLOW"
run_once "$ROOT/denied"
reap_click deny
tcc_since "$ROOT/denied" > "$ROOT/denied.tcc.log"

DENY_RESULT=$(decision_for "$ROOT/denied.tcc.log")
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

tccutil reset AudioCapture "$BUNDLE_ID" >/dev/null 2>&1 || true

{
  echo; echo "## 4. TCC records afterwards"
  echo
  echo "Compare against §0. A new *path* entry here means something got ad-hoc"
  echo "signed during the run — that record is permanent (\`tccutil reset\` cannot"
  echo "reach it) and the next run's baseline is dirty."
  echo; echo '```'
  tcc_records
  echo '```'
  echo
  echo "Signing leaf at end of run: \`$(security find-certificate -c "$IDENTITY" -p "$KEYCHAIN" 2>/dev/null | openssl x509 -outform der 2>/dev/null | shasum -a 1 | awk '{print $1}')\`"
} >> "$REPORT"

say "done — $REPORT"
cat "$REPORT"
