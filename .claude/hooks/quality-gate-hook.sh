#!/usr/bin/env bash
# quality-gate-hook.sh — Stop / SubagentStop hook: run scripts/quality-gate.sh
# on the files this run edited, and keep the run going until they pass.
#
# Claude Code pipes the hook input as JSON on stdin. Fields used (all optional;
# a missing field never breaks the hook):
#   hook_event_name        "Stop" or "SubagentStop"
#   session_id             keys the retry counter
#   agent_id               SubagentStop only; gives each sub-agent its own counter
#   transcript_path        the session's JSONL transcript
#   agent_transcript_path  SubagentStop only; the sub-agent's own transcript
#   stop_hook_active       true when this stop comes right after a hook blocked
#   cwd                    where the session runs; used to find the repo root
#
# Exit codes (what Claude Code does with them):
#   0  let the run stop
#   2  block the stop; stderr goes back to the model, which fixes and retries
#
# Loop guard: at most 2 blocking retries in a row per session (per sub-agent
# for SubagentStop). On the third failing stop the hook lets the run end with
# a note, so a problem the model cannot fix never traps a session. The counter
# lives in ${TMPDIR:-/tmp}/meet-ai-quality-gate/ and resets on a pass.
#
# A bug in this hook must never wedge a session: every internal problem ends
# in exit 0 with a warning. Only a real gate failure (exit 2) blocks.
#
# Env knobs: QUALITY_GATE_DISABLE=1 turns the hook off.
# QUALITY_GATE_SKIP_TESTS=1 is passed through to the gate.

set -u

MAX_RETRIES=2

note() { echo "quality-gate-hook: $*" >&2; }

[ "${QUALITY_GATE_DISABLE:-0}" = 1 ] && exit 0

input=$(cat 2>/dev/null) || input=""

# json_get KEY — print a top-level field as text ("true"/"false" for booleans,
# empty when missing).
json_get() {
  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$input" | jq -r --arg k "$1" \
      'if type == "object" and has($k) and .[$k] != null then .[$k] | tostring else empty end' 2>/dev/null
  elif command -v python3 >/dev/null 2>&1; then
    printf '%s' "$input" | python3 -c '
import json, sys
try:
    v = json.load(sys.stdin).get(sys.argv[1])
except Exception:
    v = None
if isinstance(v, bool):
    print("true" if v else "false")
elif v is not None:
    print(v)
' "$1" 2>/dev/null
  fi
}

main() {
  local event session agent transcript agent_transcript stop_active cwd root gate
  local state_dir key counter count files sig t rc

  if ! command -v jq >/dev/null 2>&1 && ! command -v python3 >/dev/null 2>&1; then
    note "neither jq nor python3 is installed; cannot read the hook input, skipping"
    return 0
  fi

  event=$(json_get hook_event_name)
  session=$(json_get session_id)
  agent=$(json_get agent_id)
  transcript=$(json_get transcript_path)
  agent_transcript=$(json_get agent_transcript_path)
  stop_active=$(json_get stop_hook_active)
  cwd=$(json_get cwd)
  [ -n "$cwd" ] && [ -d "$cwd" ] || cwd=$PWD

  root=$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null) || {
    note "not inside a git repository ($cwd), skipping"
    return 0
  }
  cd "$root" || return 0
  gate="$root/scripts/quality-gate.sh"
  [ -x "$gate" ] || return 0

  # Which transcript lists this run's edits.
  t=""
  if [ "$event" = SubagentStop ] && [ -n "$agent_transcript" ] && [ -f "$agent_transcript" ]; then
    t=$agent_transcript
  elif [ -n "$transcript" ] && [ -f "$transcript" ]; then
    t=$transcript
  fi
  if [ -z "$t" ]; then
    note "no transcript in the hook input; cannot tell which files this run changed, skipping"
    return 0
  fi

  # Retry counter, one per session (and per sub-agent).
  state_dir="${TMPDIR:-/tmp}/meet-ai-quality-gate"
  mkdir -p "$state_dir" 2>/dev/null || return 0
  key=${session:-unknown}
  if [ "$event" = SubagentStop ] && [ -n "$agent" ]; then
    key="$key.$agent"
  fi
  key=$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')
  counter="$state_dir/$key"
  count=$(cat "$counter" 2>/dev/null)
  case $count in
    '' | *[!0-9]*) count=0 ;;
  esac
  # stop_hook_active=false means this stop was not caused by a hook block: a
  # fresh turn, so it gets fresh retries.
  [ "$stop_active" = false ] && count=0
  if [ "$count" -ge "$MAX_RETRIES" ]; then
    note "the quality gate still fails after $MAX_RETRIES retries; letting this run stop. Run scripts/quality-gate.sh by hand to see what is left."
    rm -f "$counter"
    return 0
  fi

  files=$("$gate" --list --from-transcript "$t" 2>/dev/null) || {
    note "could not list changed files, skipping"
    return 0
  }
  if [ -z "$files" ]; then
    rm -f "$counter"
    return 0
  fi

  # Skip the run when nothing changed since the last pass (same files, same
  # contents, same HEAD). Later turns of a long session stay fast.
  sig=$({
    git rev-parse HEAD 2>/dev/null
    printf '%s\n' "$files" | while IFS= read -r f; do
      printf '%s ' "$f"
      shasum <"$f" 2>/dev/null
    done
  } | shasum | cut -d' ' -f1)
  if [ -n "$sig" ] && [ "$(cat "$counter.pass" 2>/dev/null)" = "$sig" ]; then
    rm -f "$counter"
    return 0
  fi

  set --
  while IFS= read -r f; do
    set -- "$@" "$f"
  done <<EOF
$files
EOF

  "$gate" "$@" >&2
  rc=$?
  case $rc in
    0)
      rm -f "$counter"
      [ -n "$sig" ] && printf '%s\n' "$sig" >"$counter.pass"
      return 0
      ;;
    2)
      count=$((count + 1))
      printf '%s\n' "$count" >"$counter"
      echo "(quality gate block $count of $MAX_RETRIES for this run)" >&2
      return 2
      ;;
    *)
      note "scripts/quality-gate.sh exited $rc (an internal error, not a failed check); not blocking"
      return 0
      ;;
  esac
}

(main)
rc=$?
if [ "$rc" = 2 ]; then
  exit 2
fi
if [ "$rc" != 0 ]; then
  note "hook error (exit $rc); not blocking"
fi
exit 0
