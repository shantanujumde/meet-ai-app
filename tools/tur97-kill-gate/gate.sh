#!/usr/bin/env bash
# TUR-97 gate: does a recording survive the app dying mid-meeting?
#
# Starts a real recording in the real app, lets it run, ends it the hard way
# (kill -9) or the normal way (Quit while recording), relaunches the app, and
# then checks the files a user would open: both WAVs play for (nearly) as long
# as we recorded, segments.json is there, drift-check can measure it, and the
# system track transcribes to the sentence we played through the speakers.
#
#   tools/tur97-kill-gate/gate.sh                          # 60 s, kill -9
#   tools/tur97-kill-gate/gate.sh --seconds 3600           # the 1-hour variant
#   tools/tur97-kill-gate/gate.sh --end quit               # Quit mid-recording
#   tools/tur97-kill-gate/gate.sh --verify-only <meeting>  # after a manual lid
#                                                          # close / logout / sleep
#
# See README.md next to this file for every option and what each check proves.
# Written for macOS /bin/bash 3.2: no mapfile, no associative arrays.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
CHECK="$HERE/check.py"

APP="/Applications/meet-ai.app"
REC_SECONDS=60
END_MODE="kill9"
OUT=""
KEEP=""
VERIFY_ONLY=""
EXPECT_SECONDS=""
STT="$ROOT/target/meet-stt"
REAL_ROOT_ONLY=""
SENTENCE="The quick brown fox jumps over the lazy dog while the recorder keeps running."
LOG_FILE="$HOME/Library/Logs/pro.saleschat.meetai/meet-ai.log"

usage() {
  sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
  cat <<EOF

Options:
  --app PATH             app bundle to test (default $APP)
  --seconds N            how long to record before ending it (default 60)
  --end kill9|quit       kill9 = kill -9 the app; quit = normal Quit while recording
  --out DIR              evidence folder (default target/tur97-kill-gate/<time>-<end>)
  --keep                 keep the test recording even when every check passes
  --verify-only DIR      skip recording; relaunch the app and check meeting DIR
  --expect-seconds N     with --verify-only: how long that recording ran
  --stt PATH             meet-stt binary (default $ROOT/target/meet-stt; \`just sidecar\`)
  --real-root            record into the app's real meetings folder instead of a
                         temp one (the folder is moved into --out afterwards)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --seconds) REC_SECONDS="$2"; shift 2 ;;
    --end) END_MODE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --verify-only) VERIFY_ONLY="$2"; shift 2 ;;
    --expect-seconds) EXPECT_SECONDS="$2"; shift 2 ;;
    --stt) STT="$2"; shift 2 ;;
    --real-root) REAL_ROOT_ONLY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1 (see --help)" >&2; exit 64 ;;
  esac
done

case "$END_MODE" in kill9|quit) ;; *) echo "--end must be kill9 or quit" >&2; exit 64 ;; esac
[[ "$REC_SECONDS" =~ ^[0-9]+$ ]] || { echo "--seconds must be a whole number" >&2; exit 64; }
[[ -d "$APP" ]] || { echo "no app bundle at $APP" >&2; exit 1; }
APP="$(cd "$APP" && pwd)"

if [[ -z "$OUT" ]]; then
  tag="$END_MODE-${REC_SECONDS}s"
  [[ -n "$VERIFY_ONLY" ]] && tag="verify"
  OUT="$ROOT/target/tur97-kill-gate/$(date +%Y%m%d-%H%M%S)-$tag"
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
SUMMARY="$OUT/summary.txt"
: >"$SUMMARY"

# --------------------------------------------------------------------------
# helpers

say_line() { echo "$*" | tee -a "$SUMMARY"; }
note() { echo "   $(date +%H:%M:%S) $*" | tee -a "$OUT/run.log"; }

PASSED=0
TOTAL=0
FAILED_ANY=""
# report ok|FAIL|skip <check name> <reason>
report() {
  local status="$1" name="$2" reason="$3"
  case "$status" in
    ok) PASSED=$((PASSED + 1)); TOTAL=$((TOTAL + 1)) ;;
    FAIL) TOTAL=$((TOTAL + 1)); FAILED_ANY=1 ;;
    skip) ;;
  esac
  say_line "$(printf '%-4s  %-34s %s' "$status" "$name" "$reason")"
}

# Only processes running from the bundle under test count as "the app".
# Other agents or the user may run a different meet-ai build at the same
# time, and the gate must never quit or kill those.
all_meet_ai() { pgrep -x meet-ai 2>/dev/null || true; }
app_pids() {
  local pid
  for pid in $(all_meet_ai); do
    case "$(ps -o command= -p "$pid" 2>/dev/null)" in
      "$APP/Contents/MacOS/"*) echo "$pid" ;;
    esac
  done
}
app_running() { [[ -n "$(app_pids)" ]]; }
other_meet_ai() {
  local pid mine; mine=" $(app_pids | tr '\n' ' ') "
  for pid in $(all_meet_ai); do
    case "$mine" in *" $pid "*) ;; *) echo "$pid" ;; esac
  done
}
file_size() { stat -f %z "$1" 2>/dev/null || echo 0; }
now() { date +%s; }

# wait_for <seconds> <command...>: poll every 0.5 s until the command succeeds.
wait_for() {
  local secs="$1"; shift
  local tries=$((secs * 2))
  while [[ $tries -gt 0 ]]; do
    "$@" && return 0
    sleep 0.5
    tries=$((tries - 1))
  done
  return 1
}
app_gone() { ! app_running; }

# run_timeout <seconds> <command...>: run it, kill -9 it if it outlives the limit.
# Exit status 137 means it was killed. (macOS has no `timeout`.)
run_timeout() {
  local secs="$1"; shift
  "$@" &
  local pid=$!
  ( sleep "$secs"; kill -9 "$pid" 2>/dev/null ) >/dev/null 2>&1 &
  local watchdog=$!
  local rc=0
  wait "$pid" || rc=$?
  kill "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true
  return $rc
}

# json_field <json line> <python expression over d>
json_field() { python3 -c 'import json,sys; d=json.loads(sys.argv[1]); v='"$2"'; print("" if v is None else v)' "$1"; }

# Normal Quit, as if from the menu. Only sent while the app runs, because
# `tell application` launches an app that is not running.
quit_app() {
  app_running || return 0
  # By path, not bundle id: every meet-ai build shares the id, and a quit by
  # id could land on someone else's copy.
  osascript -e "tell application \"$APP\" to quit" >>"$OUT/run.log" 2>&1 || true
  wait_for "${1:-20}" app_gone
}

# The meetings folder the app uses when nothing overrides it (meetings.rs root()).
real_root() {
  python3 - <<'EOF'
import json, os
p = os.path.expanduser("~/Library/Application Support/meet-ai/root.json")
try:
    r = json.load(open(p)).get("customRoot")
except Exception:
    r = None
print(r or os.path.expanduser("~/Meetings"))
EOF
}

list_meetings() { find "$1" -mindepth 1 -maxdepth 1 -type d -name '*-meeting*' 2>/dev/null | sort; }

SAY_PID=""
LAUNCHED=""
MEETING=""
MOVED_FROM_REAL=""
cleanup() {
  local rc=$?
  [[ -n "$SAY_PID" ]] && kill "$SAY_PID" 2>/dev/null || true
  pkill -f "say -r 170 $SENTENCE" 2>/dev/null || true
  if [[ -n "$LAUNCHED" ]] && app_running; then
    note "cleanup: meet-ai still running, quitting it"
    quit_app 20 || { note "cleanup: quit timed out, kill -9"; kill -9 $(app_pids) 2>/dev/null || true; }
  fi
  collect_meeting
  exit $rc
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# A test recording that landed in the real meetings folder (because the env
# override did not reach the app) is moved into the evidence folder, so the
# gate never leaves anything behind in the user's meetings. Only the folder
# this run created is ever touched.
collect_meeting() {
  if [[ -n "$MEETING" && -n "$MOVED_FROM_REAL" && -d "$MEETING" && "$MEETING" != "$OUT"/* ]]; then
    mkdir -p "$OUT/moved-from-real-root"
    mv "$MEETING" "$OUT/moved-from-real-root/"
    MEETING="$OUT/moved-from-real-root/$(basename "$MEETING")"
    note "moved the test recording out of the real meetings folder to $MEETING"
  fi
}

# --------------------------------------------------------------------------
# relaunch check (8): the app starts again after the ending, logs no ERROR or
# panic, and quits cleanly. Used by both modes.

relaunch_check() {
  local env_args=("$@")
  local off rc=0 reasons="" scan
  off=$(file_size "$LOG_FILE")
  note "relaunching $APP"
  local t0; t0=$(now)
  open "${env_args[@]+"${env_args[@]}"}" --stdout "$OUT/relaunch-app-stdout.log" --stderr "$OUT/relaunch-app-stderr.log" -a "$APP" >>"$OUT/run.log" 2>&1 || note "open returned an error (see run.log)"
  LAUNCHED=1
  if ! wait_for 10 app_running; then
    report FAIL "8 relaunch" "meet-ai process did not come up within 10 s of open"
    return
  fi
  local up=$(( $(now) - t0 ))
  note "relaunched (pid $(app_pids | tr '\n' ' ')) after ${up}s; letting it settle 8 s"
  sleep 8
  if ! app_running; then
    reasons="app came up but exited again within 8 s; "
    rc=1
  fi
  if ! quit_app 20; then
    reasons="${reasons}did not quit within 20 s of Quit (killed); "
    kill -9 $(app_pids) 2>/dev/null || true
    rc=1
  fi
  wait_for 5 app_gone || true
  scan=$(python3 "$CHECK" logscan "$LOG_FILE" "$off" || true)
  echo "$scan" >"$OUT/relaunch-logscan.json"
  tail -c +"$((off + 1))" "$LOG_FILE" >"$OUT/relaunch-app-log.txt" 2>/dev/null || true
  if [[ "$(json_field "$scan" 'd["ok"]')" != "True" ]]; then
    rc=1
  fi
  reasons="${reasons}up in ${up}s, $(json_field "$scan" 'd["reason"]')"
  if [[ $rc -eq 0 ]]; then report ok "8 relaunch" "$reasons, quit cleanly"
  else report FAIL "8 relaunch" "$reasons"; fi
}

# --------------------------------------------------------------------------
# file checks (1-7) on one meeting folder

verify_meeting() {
  local dir="$1" expect="$2" audio="$1/audio"
  if [[ -d "$audio" ]]; then
    report ok "1 meeting folder" "$dir"
  else
    report FAIL "1 meeting folder" "no audio/ folder in $dir"
  fi

  # 2-4: both WAVs, what the header declares versus what is on disk.
  local ch json hdr_ok=1 hdr_why="" dur_ok=1 dur_why="" tail_ok=1 tail_why=""
  local mic_declared=0
  for ch in mic system; do
    local wav="$audio/$ch.wav"
    if [[ ! -f "$wav" ]]; then
      hdr_ok=""; hdr_why="$hdr_why$ch.wav missing; "
      dur_ok=""; dur_why="$dur_why$ch.wav missing; "
      tail_ok=""; tail_why="$tail_why$ch.wav missing; "
      continue
    fi
    if [[ -n "$expect" ]]; then
      json=$(python3 "$CHECK" wav "$wav" --expect-seconds "$expect" || true)
    else
      json=$(python3 "$CHECK" wav "$wav" || true)
    fi
    echo "$json" >"$OUT/final-$ch-wav.json"
    xxd -l 64 "$wav" >"$OUT/final-$ch-header.hex" 2>/dev/null || true
    afinfo "$wav" >"$OUT/final-$ch-afinfo.txt" 2>&1 || true
    if [[ "$(json_field "$json" 'd.get("header_ok")')" != "True" ]]; then
      hdr_ok=""
      hdr_why="$hdr_why$ch: $(json_field "$json" 'd.get("header_reason") or d.get("reason")'); "
    else
      hdr_why="$hdr_why$ch: $(json_field "$json" 'd["header_reason"]'); "
    fi
    local dok; dok=$(json_field "$json" 'd.get("duration_ok")')
    [[ "$dok" == "False" ]] && dur_ok=""
    dur_why="$dur_why$ch: $(json_field "$json" 'd.get("duration_reason") or d.get("reason")'); "
    [[ "$(json_field "$json" 'd.get("tail_ok")')" == "True" ]] || tail_ok=""
    tail_why="$tail_why$ch: $(json_field "$json" 'd.get("tail_reason") or d.get("reason")'); "
    [[ "$ch" == mic ]] && mic_declared=$(json_field "$json" 'd.get("declared_s", 0)')
  done
  if [[ -n "$hdr_ok" ]]; then report ok "2 WAV headers parse + consistent" "${hdr_why%; }"
  else report FAIL "2 WAV headers parse + consistent" "${hdr_why%; }"; fi
  if [[ -z "$expect" ]]; then report skip "3 declared duration" "no --expect-seconds given"
  elif [[ -n "$dur_ok" ]]; then report ok "3 declared duration" "${dur_why%; }"
  else report FAIL "3 declared duration" "${dur_why%; }"; fi
  if [[ -n "$tail_ok" ]]; then report ok "4 PCM beyond header < 6 s" "${tail_why%; }"
  else report FAIL "4 PCM beyond header < 6 s" "${tail_why%; }"; fi

  # 5: segments.json
  json=$(python3 "$CHECK" segments "$audio/segments.json" || true)
  echo "$json" >"$OUT/final-segments-check.json"
  [[ -f "$audio/segments.json" ]] && cp "$audio/segments.json" "$OUT/final-segments.json"
  if [[ "$(json_field "$json" 'd["ok"]')" == "True" ]]; then report ok "5 segments.json" "$(json_field "$json" 'd["reason"]')"
  else report FAIL "5 segments.json" "$(json_field "$json" 'd["reason"]')"; fi

  # 6: drift-check. Exit 0 is required. Exit 2 ("not measurable") is accepted
  # only for a recording too short to have reached its first 5 s checkpoint,
  # where "no checkpoint anchors" is the honest answer. Any other exit 2 after
  # a kill means the crash lost the anchors drift is measured from.
  local drc=0 dout
  (cd "$ROOT" && cargo run -q -p audio --bin drift-check -- "$audio") \
    >"$OUT/drift-check.out" 2>"$OUT/drift-check.err" || drc=$?
  dout="$( (tr '\n' ' ' <"$OUT/drift-check.out"; grep -v '^ *$' "$OUT/drift-check.err" | tr '\n' ' ') | sed 's/  */ /g' || true)"
  local short=""
  python3 -c "import sys; sys.exit(0 if float(sys.argv[1]) < 10 else 1)" "${expect:-$mic_declared}" && short=1
  if grep -q "invariant violation" "$OUT/drift-check.err"; then
    report FAIL "6 drift-check" "exit $drc, $(grep -m1 'invariant violation' "$OUT/drift-check.err")"
  elif [[ $drc -eq 0 ]]; then
    report ok "6 drift-check" "$(grep -m1 'PASS' "$OUT/drift-check.out" || echo 'exit 0')"
  elif [[ $drc -eq 2 && -n "$short" ]] && grep -q "no checkpoint anchors" "$OUT/drift-check.err"; then
    report ok "6 drift-check" "exit 2 accepted: recording shorter than 10 s has no checkpoint yet"
  elif [[ $drc -eq 2 ]]; then
    report FAIL "6 drift-check" "exit 2 (not measurable): $(grep -m1 -E 'not measurable|could not' "$OUT/drift-check.err" || echo "$dout")"
  elif [[ $drc -eq 1 ]]; then
    report FAIL "6 drift-check" "exit 1 (over the drift gate): $dout"
  else
    report FAIL "6 drift-check" "exit $drc, could not run (see drift-check.err): $(tail -1 "$OUT/drift-check.err")"
  fi

  # 7: the system track transcribes to the sentence we played. meet-stt reads
  # the file the way a player does -- through the header -- so a header that
  # declares nothing gives nothing back. It has been seen to hang forever on
  # such a file, hence the time limit.
  if [[ ! -x "$STT" ]]; then
    report FAIL "7 system.wav transcribes" "meet-stt not found at $STT (build it: just sidecar, or pass --stt)"
  elif [[ ! -f "$audio/system.wav" ]]; then
    report FAIL "7 system.wav transcribes" "system.wav missing"
  else
    local limit src=0
    limit=$(python3 -c "import sys; print(int(max(60, float(sys.argv[1]) * 1.5)))" "${expect:-60}")
    run_timeout "$limit" "$STT" "$audio/system.wav" >"$OUT/stt-system.jsonl" 2>"$OUT/stt-system.err" || src=$?
    json=$(python3 "$CHECK" words "$OUT/stt-system.jsonl" "$SENTENCE" || true)
    echo "$json" >"$OUT/stt-system-check.json"
    if [[ $src -eq 137 ]]; then
      report FAIL "7 system.wav transcribes" "meet-stt hung and was killed after ${limit}s; $(json_field "$json" 'd["reason"]')"
    elif [[ "$(json_field "$json" 'd["ok"]')" == "True" ]]; then
      report ok "7 system.wav transcribes" "$(json_field "$json" 'd["reason"]')"
    else
      report FAIL "7 system.wav transcribes" "meet-stt exit $src; $(json_field "$json" 'd["reason"]')"
    fi
  fi
}

finish() {
  say_line ""
  say_line "$PASSED/$TOTAL passed"
  say_line "evidence: $OUT"
  if app_running; then
    say_line "WARNING: the meet-ai under test is still running: $(app_pids | tr '\n' ' ')"
    FAILED_ANY=1
  fi
  [[ -z "$FAILED_ANY" ]]
}

# --------------------------------------------------------------------------
# preflight

command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }
if [[ -n "$(all_meet_ai)" ]]; then
  echo "meet-ai is already running (pid $(all_meet_ai | tr '\n' ' ')). Quit it first --" >&2
  echo "the gate has to own the only running copy to know which process to end." >&2
  trap - EXIT
  exit 1
fi

say_line "TUR-97 kill gate — $(date '+%Y-%m-%d %H:%M:%S')"
say_line "app: $APP ($(defaults read "$APP/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo '?'))"

echo "==> building drift-check (cargo build -p audio --bin drift-check)"
if ! (cd "$ROOT" && cargo build -q -p audio --bin drift-check) >"$OUT/drift-check-build.log" 2>&1; then
  note "drift-check did not build; check 6 will fail (see drift-check-build.log)"
fi

# --------------------------------------------------------------------------
# --verify-only: the recording was ended by hand (lid close, logout, sleep).

if [[ -n "$VERIFY_ONLY" ]]; then
  [[ -d "$VERIFY_ONLY" ]] || { echo "no such meeting folder: $VERIFY_ONLY" >&2; exit 1; }
  MEETING="$(cd "$VERIFY_ONLY" && pwd)"
  say_line "mode: verify-only $MEETING${EXPECT_SECONDS:+ (expected ${EXPECT_SECONDS}s)}"
  say_line ""
  # Relaunch against the folder the meeting lives in, so a recovery-on-launch
  # fix sees it. The real root needs no override.
  env_args=()
  parent="$(dirname "$MEETING")"
  if [[ "$parent" != "$(real_root)" ]]; then
    env_args=(--env "MEET_AI_MEETINGS_ROOT=$parent")
  fi
  relaunch_check "${env_args[@]+"${env_args[@]}"}"
  verify_meeting "$MEETING" "$EXPECT_SECONDS"
  MEETING=""   # never move or delete a folder this run did not create
  finish
  exit $?
fi

# --------------------------------------------------------------------------
# record, end, relaunch, verify

REAL_ROOT="$(real_root)"
TEMP_ROOT="$OUT/meetings-root"
mkdir -p "$TEMP_ROOT/.app"
# The onboarding flag lives inside the meetings root (onboarding.rs path()),
# so a fresh temp root would open the app on its setup screen, where ⌘⇧R does
# not start a recording. Mark setup as done for the temp root only.
printf '{\n  "completedAt": "%s"\n}\n' "$(date +%Y-%m-%dT%H:%M:%S%z | sed 's/\(..\)$/:\1/')" \
  >"$TEMP_ROOT/.app/onboarding.json"
list_meetings "$REAL_ROOT" >"$OUT/real-root-before.txt"
list_meetings "$TEMP_ROOT" >"$OUT/temp-root-before.txt"

ENV_ARGS=()
if [[ -z "$REAL_ROOT_ONLY" ]]; then
  ENV_ARGS=(--env "MEET_AI_MEETINGS_ROOT=$TEMP_ROOT")
fi
say_line "mode: record ${REC_SECONDS}s, end=$END_MODE, meetings root requested: ${REAL_ROOT_ONLY:+real }${REAL_ROOT_ONLY:-$TEMP_ROOT}"
say_line ""

LOG_OFF0=$(file_size "$LOG_FILE")
note "launching $APP"
open "${ENV_ARGS[@]+"${ENV_ARGS[@]}"}" --stdout "$OUT/app-stdout.log" --stderr "$OUT/app-stderr.log" -a "$APP"
LAUNCHED=1
if ! wait_for 15 app_running; then
  report FAIL "1 meeting folder" "meet-ai did not start within 15 s of open"
  finish; exit 1
fi
PID="$(app_pids | head -1)"
note "meet-ai up (pid $PID); waiting 4 s before the shortcut"
sleep 4

if [[ -n "$(other_meet_ai)" ]]; then
  report FAIL "1 meeting folder" "another meet-ai started meanwhile (pid $(other_meet_ai | tr '\n' ' ')); it could take the Cmd+Shift+R shortcut, so not sending it"
  finish; exit 1
fi
note "sending Cmd+Shift+R"
if ! osascript -e 'tell application "System Events" to keystroke "r" using {command down, shift down}' \
    >>"$OUT/run.log" 2>&1; then
  report FAIL "1 meeting folder" "could not send Cmd+Shift+R (Accessibility permission for this terminal?) -- see run.log"
  finish; exit 1
fi

new_meeting() {
  local root
  for root in "$TEMP_ROOT" "$REAL_ROOT"; do
    local before="$OUT/temp-root-before.txt"
    [[ "$root" == "$REAL_ROOT" ]] && before="$OUT/real-root-before.txt"
    local found
    found=$(list_meetings "$root" | comm -13 "$before" - | tail -1)
    if [[ -n "$found" ]]; then
      MEETING="$found"
      [[ "$root" == "$REAL_ROOT" ]] && MOVED_FROM_REAL=1
      return 0
    fi
  done
  return 1
}
if ! wait_for 20 new_meeting; then
  report FAIL "1 meeting folder" "no new *-meeting folder within 20 s of Cmd+Shift+R (shortcut ignored, or a permission dialog is up?)"
  finish; exit 1
fi
T0=$(now)
if [[ -z "$REAL_ROOT_ONLY" ]]; then
  if [[ -n "$MOVED_FROM_REAL" ]]; then
    say_line "open --env: did NOT reach the app (recording went to $REAL_ROOT; it will be moved into $OUT)"
  else
    say_line "open --env: worked (recording went to the temp root)"
  fi
fi
note "recording started: $MEETING"
wait_for 10 test -f "$MEETING/audio/mic.wav" || note "audio/mic.wav not there after 10 s"

# Play the sentence a few times so system.wav has known speech in it: once
# right away, then every 2 minutes on a long run.
NEXT_SAY=2
LAST_PROGRESS=0
while :; do
  elapsed=$(( $(now) - T0 ))
  [[ $elapsed -ge $REC_SECONDS ]] && break
  if [[ $elapsed -ge $NEXT_SAY && $((REC_SECONDS - elapsed)) -ge 8 ]]; then
    note "say (t=${elapsed}s)"
    say -r 170 "$SENTENCE" &
    SAY_PID=$!
    NEXT_SAY=$((NEXT_SAY + 120))
  fi
  if [[ $((elapsed - LAST_PROGRESS)) -ge 60 ]]; then
    LAST_PROGRESS=$elapsed
    note "recording ${elapsed}/${REC_SECONDS}s, mic.wav $(file_size "$MEETING/audio/mic.wav") bytes"
  fi
  if ! app_running; then
    note "meet-ai exited on its own during the recording"
    break
  fi
  sleep 1
done
T_END=$(now)
RECORDED=$((T_END - T0))

if [[ "$END_MODE" == kill9 ]]; then
  note "kill -9 $PID after ${RECORDED}s"
  kill -9 "$PID" 2>/dev/null || true
  wait_for 5 app_gone || true
else
  note "Quit (normal quit, still recording) after ${RECORDED}s"
  qstart=$(now)
  if quit_app 20; then
    report ok "0 Quit while recording exits" "app exited $(( $(now) - qstart ))s after Quit"
  else
    report FAIL "0 Quit while recording exits" "still running 20 s after Quit; killed"
    kill -9 $(app_pids) 2>/dev/null || true
    wait_for 5 app_gone || true
  fi
fi
[[ -n "$SAY_PID" ]] && kill "$SAY_PID" 2>/dev/null || true
SAY_PID=""
LAUNCHED=""

# What the ending left on disk, before the relaunch gets a chance to repair it.
for ch in mic system; do
  if [[ -f "$MEETING/audio/$ch.wav" ]]; then
    python3 "$CHECK" wav "$MEETING/audio/$ch.wav" --expect-seconds "$RECORDED" \
      >"$OUT/after-end-$ch-wav.json" || true
    xxd -l 64 "$MEETING/audio/$ch.wav" >"$OUT/after-end-$ch-header.hex" 2>/dev/null || true
  fi
done
[[ -f "$MEETING/audio/segments.json" ]] && cp "$MEETING/audio/segments.json" "$OUT/after-end-segments.json"
/bin/ls -la "$MEETING" "$MEETING/audio" >"$OUT/after-end-ls.txt" 2>&1 || true
tail -c +"$((LOG_OFF0 + 1))" "$LOG_FILE" >"$OUT/recording-app-log.txt" 2>/dev/null || true

relaunch_check "${ENV_ARGS[@]+"${ENV_ARGS[@]}"}"
LAUNCHED=""
/bin/ls -la "$MEETING" "$MEETING/audio" >"$OUT/after-relaunch-ls.txt" 2>&1 || true
if ! cmp -s "$OUT/after-end-ls.txt" "$OUT/after-relaunch-ls.txt"; then
  note "the relaunch changed the meeting folder (see after-end-ls.txt vs after-relaunch-ls.txt)"
fi

verify_meeting "$MEETING" "$RECORDED"

collect_meeting
if [[ -z "$FAILED_ANY" && -z "$KEEP" ]]; then
  rm -rf "$MEETING"
  note "all checks passed; deleted the test recording (use --keep to keep it)"
else
  note "test recording kept at $MEETING"
fi
MEETING=""
finish
