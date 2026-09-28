# TUR-73 — live checks to run after the server restart

## Results (28 Sep, server restarted 20:14 on all 14 edits)

| # | Check | Result |
|---|-------|--------|
| 1 | `board` as unblock owner | **passes** — HTTP 200 on a genuinely blocked issue |
| 2 | One PATCH clears a blocker and moves off `blocked` | **passes** — HTTP 200, TUR-126 went `blocked` → `todo` in one call |
| 3 | New child not born blocked | pending — armed, see the watchdog probe below |
| 4 | Watchdog run gets a second write | pending — armed, see the watchdog probe below |

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
confirms the 14 edits are present and parse. These are the checks that need a
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
ps -axo pid=,lstart=,command= | grep 'paperclipai onboard' | grep -v grep
```

The start time must be *after* the patch was applied. `apply.mjs --check`
reporting `already-applied` says nothing about the running process — Node
caches modules at first import.

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

## Known limits of the fix

- There is a millisecond-wide race where an outside edit landing between a
  watchdog write and the re-pin would be absorbed instead of rejected. Closing
  it needs to happen inside the database transaction, which a patch at this
  layer cannot reach.
- The patch refuses any `@paperclipai/server` version other than 2026.916.1.
  An upgrade lands a fresh unpatched copy in a new npx cache folder; re-run
  `node tools/paperclip-tur73/apply.mjs` and restart.
