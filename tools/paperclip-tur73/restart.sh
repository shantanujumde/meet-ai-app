#!/bin/bash
# Restart the local Paperclip server so patched code is actually loaded.
#
# Node caches modules at first import, so tools/paperclip-tur73/apply.mjs only
# takes effect after the server process is replaced. This script does that.
#
# Usage: tools/paperclip-tur73/restart.sh [delay-seconds]
#
# The delay exists because the restart kills every in-flight agent run on this
# instance -- including the run that launched this script. Launch it detached
# (nohup ... &) with enough delay for your own run to finish writing first.
#
# Everything it does is appended to .paperclip/tur73-restart.log.
# Written for macOS /bin/bash 3.2, so no mapfile and no associative arrays.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LOG="$ROOT/.paperclip/tur73-restart.log"
SERVER_LOG="$ROOT/.paperclip/paperclip-server.log"
DELAY="${1:-120}"
HEALTH="http://127.0.0.1:3100/api/health"

exec >>"$LOG" 2>&1
echo "=== TUR-73 restart started $(date) (delay ${DELAY}s) ==="

sleep "$DELAY"

# Find the running server by its command line rather than a hardcoded pid, so
# this stays correct if it was restarted by hand in the meantime. macOS pgrep
# does not see these argv strings, hence ps.
pids_matching() {
  ps -axo pid=,command= | grep -E "$1" | grep -v grep | awk '{print $1}' | tr '\n' ' '
}

PIDS="$(pids_matching 'node .*paperclipai onboard')"
WRAPPERS="$(pids_matching 'npm exec paperclipai@')"
echo "server pids: ${PIDS:-none}  wrapper pids: ${WRAPPERS:-none}"

for pid in $WRAPPERS $PIDS; do
  kill -TERM "$pid" 2>/dev/null && echo "sent TERM to $pid"
done

for _ in $(seq 1 40); do
  still=0
  for pid in $PIDS; do kill -0 "$pid" 2>/dev/null && still=1; done
  [ "$still" -eq 0 ] && break
  sleep 1
done

for pid in $WRAPPERS $PIDS; do
  if kill -0 "$pid" 2>/dev/null; then
    echo "$pid ignored TERM, sending KILL"
    kill -KILL "$pid" 2>/dev/null
  fi
done
sleep 3

# The embedded postgres is a separate process. If the server did not take it
# down, the fresh instance will fail to claim its port.
PG="$(pids_matching 'embedded-postgres.*bin/postgres -D')"
if [ -n "$PG" ]; then
  echo "embedded postgres still up ($PG); stopping it"
  for pid in $PG; do kill -TERM "$pid" 2>/dev/null; done
  sleep 6
fi

start_server() {
  cd "$ROOT" || return 1
  nohup bin/paperclip onboard --yes --no-install-service >>"$SERVER_LOG" 2>&1 &
  echo "started server, pid $!"
}

wait_healthy() {
  i=0
  while [ "$i" -lt 90 ]; do
    code="$(curl -s -m 2 -o /dev/null -w '%{http_code}' "$HEALTH")"
    if [ "$code" = "200" ]; then
      echo "health OK after $((i * 2))s"
      return 0
    fi
    i=$((i + 1))
    sleep 2
  done
  echo "server did not answer $HEALTH within 180s -- see $SERVER_LOG"
  return 1
}

start_server
wait_healthy
healthy=$?

# npx can re-extract the package into the cache on start, which would silently
# drop the patch. Re-apply and restart once if that happened.
check="$(node "$ROOT/tools/paperclip-tur73/apply.mjs" --check 2>&1)"
echo "$check"
if echo "$check" | grep -q 'would-apply'; then
  echo "npx replaced the patched files; re-applying and restarting once more"
  node "$ROOT/tools/paperclip-tur73/apply.mjs"
  PIDS="$(pids_matching 'node .*paperclipai onboard')"
  for pid in $PIDS; do kill -TERM "$pid" 2>/dev/null; done
  sleep 10
  start_server
  wait_healthy
  healthy=$?
fi

# Run the live checks that need a running patched server, and log the answers.
# The launching run is dead by now, so nobody is around to read a return value;
# the log is the record. PROBE_BLOCKED_ISSUE must be an issue that is genuinely
# `blocked`, because unblockDescriptor is validated for blocked status before
# the owner is looked at -- on any other issue these checks pass vacuously.
# PROBE_AGENT_ID must be set too: moving an issue to `todo` also runs a
# follow-up guard that 409s with "requires an assigned agent" on an unassigned
# issue, which masks check 2 and reads as a failure of the patch.
probe() {
  if [ -z "${PROBE_TOKEN:-}" ] || [ -z "${PROBE_BLOCKED_ISSUE:-}" ]; then
    echo "probe skipped: PROBE_TOKEN/PROBE_BLOCKED_ISSUE not set"
    return
  fi
  api="http://127.0.0.1:3100/api/issues/$PROBE_BLOCKED_ISSUE"
  echo "--- live check 1: board as unblock owner ---"
  curl -s -m 10 -w '\nHTTP %{http_code}\n' -X PATCH \
    -H "Authorization: Bearer $PROBE_TOKEN" -H 'Content-Type: application/json' \
    -d '{"unblockDescriptor":{"owner":"board","action":"TUR-73 live check"}}' \
    "$api" | tail -c 200
  if [ -n "${PROBE_AGENT_ID:-}" ]; then
    echo "--- check 2 precondition: give the probe an assignee ---"
    curl -s -m 10 -w '\nHTTP %{http_code}\n' -X PATCH \
      -H "Authorization: Bearer $PROBE_TOKEN" -H 'Content-Type: application/json' \
      -d "{\"assigneeAgentId\":\"$PROBE_AGENT_ID\"}" "$api" | tail -c 80
  else
    echo "--- check 2 precondition SKIPPED: PROBE_AGENT_ID not set; a 409 below may be the assignee guard, not the patch ---"
  fi
  echo "--- live check 2: one PATCH clears a blocker and moves off blocked ---"
  curl -s -m 10 -w '\nHTTP %{http_code}\n' -X PATCH \
    -H "Authorization: Bearer $PROBE_TOKEN" -H 'Content-Type: application/json' \
    -d '{"blockedByIssueIds":[],"status":"todo"}' \
    "$api" | tail -c 200
  echo "--- resulting state ---"
  curl -s -m 10 -H "Authorization: Bearer $PROBE_TOKEN" "$api" \
    | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const i=JSON.parse(s);console.log(JSON.stringify({identifier:i.identifier,status:i.status,blockedBy:(i.blockedBy||[]).map(b=>b.identifier)}))}catch(e){console.log("unparsed: "+s.slice(0,200))}})'
}

if [ "$healthy" -eq 0 ]; then
  probe
  echo "=== TUR-73 restart finished OK $(date) ==="
else
  echo "=== TUR-73 restart FAILED $(date) -- start it by hand: bin/paperclip onboard --yes --no-install-service ==="
fi
