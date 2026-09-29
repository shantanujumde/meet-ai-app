# TUR-73 — live checks to run after the server restart

## Results

Checks 1–4 were taken on the 20:14 server (14 edits). Checks 1 and 2 were
re-taken on the 21:04 server (16 edits) to confirm the last two edits did not
regress them.

| # | Check | Result |
|---|-------|--------|
| 1 | `board` as unblock owner | **passes** — HTTP 200 on a genuinely blocked issue; re-confirmed on 16 edits (TUR-136) |
| 2 | One PATCH clears a blocker and moves off `blocked` | **passes** — HTTP 200 in one call; re-confirmed on 16 edits (TUR-136 `blocked` → `todo`) |
| 3 | New child not born blocked | **passes** — TUR-133 created under TUR-125 with `blockedBy: []` while TUR-131 sat open |
| 4 | Watchdog run gets a second write | **passes** — write 2 of the TUR-125 pass returned 200, no stale-fingerprint 409 |
| 5 | A run may write after its own liveness flip | armed — waiting on the TUR-135 watchdog pass |
| 6 | A watchdog can comment on the tree it repaired | armed — waiting on the TUR-135 watchdog pass |

Checks 5 and 6 are defects the TUR-125 watchdog pass exposed *after* 1–4
passed: writes 3, 4 and 5 of that pass all 409'd with a different message
(`currentState: live, currentStopFingerprint: null`) from a second guard behind
the first. Two more edits cover them; see checks 5 and 6 below.

Check 2 had failed on the 19:58 server because the original defect-4 edit only
covered the explicit-resume branch. A plain agent PATCH moving an issue off
`blocked` takes a different route: it needs "resume authority", which calls
`assertExplicitResumeIntentAllowed`, and that ran its own readiness check
against the *stored* blocker set. A 14th edit covers that site, and the 20:14
server confirms it live.

**Check 2 has a second precondition, learned the hard way.** The probe issue
must have an assigned agent. Moving an issue to `todo` also runs a follow-up
guard that returns `409 Issue follow-up requires an assigned agent` — a
different 409 from a different guard, on an unassigned issue it fires first and
reads exactly like the defect still being present. `restart.sh` now sets an
assignee first when `PROBE_AGENT_ID` is given, and says so loudly when it is not.

### The watchdog probe for checks 3 and 4

Checks 3 and 4 both run inside `resolveWatchdogFollowUpSerializationContext`,
which is only non-null for a watchdog run, so no ordinary API call reaches
them. TUR-21's children are all closed and TUR-115 is `done` with its watchdog
removed, so both earlier observation points are spent.

A purpose-built one is now installed: a watchdog on **TUR-125** (watchdog record
`36f6b586-b36a-43a3-8e10-a56598b531f0`), whose subtree is deliberately stopped,
with **TUR-131** as an open unassigned sibling standing in for TUR-28. When it
wakes it creates a child under TUR-125 and then immediately makes a second write
in the same subtree. Check 3 passes if the child comes back *without* TUR-131 as
a blocker; check 4 passes if the second write is not a stale-fingerprint 409.

Tear down afterwards: `DELETE /api/issues/TUR-125/watchdog`, then delete probes
TUR-125, TUR-126, TUR-131.

Everything in `verify.mjs` is an offline check: it reads the patched files and
confirms the 16 edits are present and parse. These are the checks that need a
*running* patched server, in cost order. Run them with a fresh run token
(`$PAPERCLIP_API_KEY` is scoped to one run and dies with it).

```sh
B="${PAPERCLIP_API_URL%/}"; B="${B%/api}"
AUTH=(-H "Authorization: Bearer $PAPERCLIP_API_KEY" -H "Content-Type: application/json")
```

## 0. The server is actually the patched one

The bug that wasted a pass on 28 Sep: the restart never fired, so the process
was still the one started on 27 Sep and every "fix" was inert.

```sh
ps -axo pid=,lstart=,command= | grep -E 'paperclipai (onboard|run)' | grep -v grep
```

**Match both spellings.** The server gets started two ways — `bin/paperclip
onboard` and `paperclipai run` — and on 29 Sep it was running as `run`.
`restart.sh` only knew `onboard`, so it would have killed nothing, started a
second instance that lost the port race, and still logged `health OK`, because
the *old* server was the one answering. A 200 from `/api/health` proves a
server is up, not that it is a new one. `restart.sh` now compares the server
pids before and after and fails loudly if a pre-restart pid survived.

The start time must be *after* the patch was applied. `apply.mjs --check`
reporting `already-applied` says nothing about the running process — Node
caches modules at first import.

**And the install it would start from has to parse.** On 28 Sep all three npx
caches held a double-applied `task-watchdog-scope.js` — two copies of
`taskWatchdogObservedSignature`, a SyntaxError, a server that cannot boot. The
board stayed up only because the running process had read the file before the
second apply landed. `apply.mjs` now refuses to write a file with a duplicate
export and reports an install already in that state, but the cheap direct check
is worth keeping:

```sh
node --check "$INSTALL/dist/services/task-watchdog-scope.js"
```

## 1. `board` as an unblock owner stops returning 403

**Gotcha found on 28 Sep:** `unblockDescriptor` is validated for
`status == blocked` *before* the owner is checked. On a non-blocked issue you
get `422 unblockDescriptor requires blocked status` and never reach the code
under test. Run this against an issue that is genuinely blocked, or the check
is vacuous.

```sh
curl -s -w '\n%{http_code}\n' -X PATCH "${AUTH[@]}" \
  -d '{"unblockDescriptor":{"owner":"board","action":"needs a person"}}' \
  "$B/api/issues/$BLOCKED_ISSUE_ID"
```

Before: `403 Agents may only name themselves as an unblock owner`.
After: accepted, and the message for a *named user* should now explain itself.

## 2. One PATCH can clear a blocker and move off `blocked`

The transition used to be validated against the stored blocker set, ignoring
the one the same request was clearing, so this took two writes.

```sh
curl -s -w '\n%{http_code}\n' -X PATCH "${AUTH[@]}" \
  -d '{"blockedByIssueIds":[],"status":"todo"}' "$B/api/issues/$BLOCKED_ISSUE_ID"
```

Before: 409. After: 200, issue is `todo` with no blockers.

The 409 to watch for here is `Issue follow-up blocked by unresolved blockers`,
*not* the stale-fingerprint one from the issue report. They are different
guards that both fire on this call.

**Check 2 has a third precondition: park the probe afterwards.** A pass leaves
the probe `todo` *with* an assignee, which is precisely the shape the scheduler
picks up — it starts a real run on a throwaway issue within seconds. Once that
run holds the checkout, every status write (`cancelled`, `done`, `backlog`,
re-block) comes back `409 Issue run ownership conflict`, and the probe cannot be
reclaimed at all; it has to be talked down with a comment or cancelled from the
board. This happened on 28 Sep with TUR-136. `restart.sh` now PATCHes the probe
back to `{"status":"backlog","assigneeAgentId":null}` straight after the check.
The window is small but not zero.

`restart.sh` now runs checks 1 and 2 itself once the server is healthy and
appends the answers to `.paperclip/tur73-restart.log`. Set `PROBE_TOKEN` to a
live API key and `PROBE_BLOCKED_ISSUE` to a genuinely blocked issue id before
launching it; without both it logs `probe skipped`.

## 3. A new child is not born blocked behind a manual sibling

TUR-21 is the reproduction: TUR-28 is a `manual` issue sitting in `in_review`,
and it was the oldest open child, so the follow-up serializer chained every new
child behind it.

```sh
curl -s -X POST "${AUTH[@]}" -d '{"title":"TUR-73 probe","description":"delete me"}' \
  "$B/api/issues/$TUR21_ID/children" | jq '{id,identifier,status,blockedBy}'
```

Before: `blockedBy: [TUR-28]`, status `blocked`, unrequested.
After: no blockers. Delete the probe issue afterwards.

## 4. A watchdog run gets a second write

The headline defect, and the only one that cannot be probed with a plain API
call — it needs a real watchdog run, because the guard compares against the
fingerprint pinned in that run's `contextSnapshot`.

Observation point rather than a probe: the TUR-30 watchdog wakes on its own.
Read its comment history after the next pass and look for two successful
mutations in one run, where previously every pass stopped at one with:

```
Task-watchdog review is stale because the watched subtree stop fingerprint changed
```

Four passes (916a2517, 142cdc37, 464dae31, 60a80c90) each stopped after exactly
one write. A pass that makes two is the proof.

**Result:** the TUR-125 watchdog pass (run `18bb0dad`, 28 Sep 20:18) got its
second write. Writes 1 and 2 both landed. Checks 3 and 4 pass.

## 5. A run may keep writing after its own write revives the subtree

What run `18bb0dad` hit next: writes 3, 4 and 5 all 409'd, but with a different
message from the one in the bug report —

```
Task-watchdog review is stale because the watched subtree now has a live,
waiting, already-reviewed, or not-applicable path
  currentState:           live
  currentStopFingerprint: null
```

The guard no longer counted the run's own fingerprint changes, but the moment
write 2 made the subtree live the verdict stopped being `stopped` at all, and
no signature could match. The run created a task and could not assign it.

Observation point, not a probe — it needs a real watchdog run. The TUR-125 rig
that proved checks 3 and 4 was torn down by its own cleanup task (TUR-133), so
a second one is installed for checks 5 and 6:

| issue | role |
|---|---|
| **TUR-135** | the watched parent; watchdog registered on it |
| TUR-136 | leftover from checks 1–2; the scheduler grabbed it, see the note above |
| TUR-137 | open unassigned sibling, standing in for TUR-28 |
| TUR-138 | blocked and unassigned, so the subtree has no live path |

Its instructions tell the pass to make five writes in order and record the HTTP
code of each on its own review issue — create a follow-up, move it to `todo`
(the write that flips the subtree live), assign it (check 5), comment on
TUR-135 (check 6), then re-read the siblings.

A pass that creates a follow-up **and** assigns it, in one run, is the proof.
The allowance is narrow on purpose: it only fires when every issue the
classifier credits for the live verdict is one this run wrote to, per the
`activity_log` run id. If someone else revived the path, the 409 stands.

## 6. A watchdog can comment on the tree it just repaired

Write 5 of run `18bb0dad` was the summary comment the watchdog mandate asks
for, on the watched issue, and it 409'd with the same liveness error. The
evidence had to be parked on the watchdog issue instead.

```sh
curl -s -w '\n%{http_code}\n' -X POST "${AUTH[@]}" \
  -d '{"body":"watchdog summary"}' "$B/api/issues/$WATCHED_ISSUE_ID/comments"
```

Before: 409 once the subtree was live. After: accepted. A comment outside the
watched subtree must still be refused — the scope check is untouched.

## 7. A new child is not born blocked behind the watchdog's own review issue

Check 3 passed on the TUR-125 rig by luck: the only sibling there was
`backlog`, and `backlog` siblings were never anchor candidates. On the TUR-135
rig the sibling that got picked was TUR-136 — the watchdog's *own review issue*,
which is a child of the watched issue and is created by the watchdog agent, so
the author filter from fix 2 matched it. TUR-140 came back blocked behind the
issue that was creating it.

Needs a real watchdog run; no ordinary API call reaches the serialization path.
The rig needs a watched issue whose children include the watchdog review issue
in `in_progress` or `in_review` — which is automatic, since the review issue is
mid-run at the moment it creates the follow-up.

```sh
# from inside a watchdog pass, under the watched issue
curl -s "${AUTH[@]}" -X POST -d '{"title":"probe follow-up"}' \
  "$B/api/issues/$WATCHED_ISSUE_ID/children"
```

Before: `status: "blocked"`, `blockedBy: [<the watchdog review issue>]`.
After: `status` as requested, `blockedBy: []`.

Serialization behind a genuine earlier follow-up must still happen — create two
follow-ups in one pass and the second should chain behind the first. A rig where
that stops working means the exclusion was applied too widely.

## 8. The child-create 201 shows the blocker edges it just wrote

Independent of check 7, and it is what made the earlier readings so slippery:
the 201 body was composed before serialization wired its edges, so it reported
`blocks: []` on a child that had just been made a blocker of its sibling. The
edge only appeared on a later GET.

```sh
CHILD=$(curl -s "${AUTH[@]}" -X POST -d '{"title":"probe"}' \
  "$B/api/issues/$WATCHED_ISSUE_ID/children")
echo "$CHILD" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["id"], d.get("blocks"), d.get("blockedBy"))'
# then re-read and compare — the two must now agree
curl -s "${AUTH[@]}" "$B/api/issues/$(echo "$CHILD" | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')"
```

Before: the 201 and the follow-up GET disagreed. After: identical.

## 9. Assign and start in one PATCH

Reachable from an ordinary run — no watchdog needed. It must be run against an
issue that is currently **unassigned**, because that is the branch that was an
unconditional 409.

```sh
curl -s -w '\n%{http_code}\n' -X PATCH "${AUTH[@]}" \
  -d "{\"assigneeAgentId\":\"$PAPERCLIP_AGENT_ID\",\"status\":\"todo\"}" \
  "$B/api/issues/$PROBE_ISSUE_ID"
```

Before: `409 Issue follow-up requires an assigned agent`. After: 200.

Also check the guard did not open too far: PATCHing an issue **already assigned
to another agent**, naming yourself in the body, must still be refused with
`Agent cannot request follow-up for another agent's issue`. The stored assignee
wins when it exists; if that check starts passing, the `??` order was inverted.

Same hazard as check 2 — an issue left `todo` with an assignee is exactly what
the scheduler picks up, and once it holds the checkout you cannot get it back.
Park the probe to `backlog`/unassigned immediately afterwards.

## Known limits of the fix

- There is a millisecond-wide race where an outside edit landing between a
  watchdog write and the re-pin would be absorbed instead of rejected. Closing
  it needs to happen inside the database transaction, which a patch at this
  layer cannot reach.
- The patch refuses any `@paperclipai/server` version other than 2026.916.1.
  An upgrade lands a fresh unpatched copy in a new npx cache folder; re-run
  `node tools/paperclip-tur73/apply.mjs` and restart.
