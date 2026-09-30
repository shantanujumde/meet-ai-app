# TUR-44 — Watchdog cannot deliver a board answer it asked for: already_reviewed blocks the subtree after the interaction resolves

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Alen |
| Created | 2026-09-27 14:29 UTC by Alen |
| Completed | 2026-09-28 12:31 UTC |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Description

#### What happened

A task watchdog asked the board a question, the board answered it, and the watchdog was then structurally unable to deliver the answer to the issue it was watching.

Concretely, on TUR-26 (watchdog for TUR-13):

1. TUR-13 went `blocked`. The watchdog reviewed it at stop fingerprint `c57c252d…`, could not resolve the disposition alone, and created an `ask_user_questions` interaction on its own watchdog issue asking the board how to proceed. It then ended the run with a `snooze` decision, expecting the next wake to carry a fresh fingerprint.
2. The board answered at 14:25:13 — *"unblock TUR-13 and let Vox finish the two leftovers there"*, plus *"reformat the fixtures, do not add a biome exclusion."*
3. The watchdog woke on `wake_assignee` as designed. But TUR-13's stop fingerprint was **still** `c57c252d…`, so the subtree guard classified the state as `already_reviewed` and refused every write:

```
HTTP 409
"Task-watchdog review is stale because the watched subtree now has a live, waiting,
 already-reviewed, or not-applicable path; refresh the source state before mutating it."
currentState: "already_reviewed"
runStopFingerprint:     task_watchdog_stop:c57c252d…
currentStopFingerprint: task_watchdog_stop:c57c252d…
```

Refused: `POST /api/issues/{TUR-13}/comments`, and `POST /api/companies/{id}/issues` with `parentId = TUR-13`. Both are on the watchdog's own allowed-operations list (`comment_on_watched_subtree_issues`, `create_child_issues_under_non_watchdog_watched_subtree`).

#### Why this is a bug and not the guard working

The guard is right in general — it stops a watchdog looping on unchanged state. But the state *did* change in the only way that mattered: **the human decision the watchdog was waiting for arrived.** The fingerprint does not include it, because it is computed from the watched subtree's stopped leaves, and the interaction lives on the watchdog issue.

The result is a closed loop with no exit:

- the watchdog cannot act, because it already reviewed this fingerprint;
- nothing will change the fingerprint, because the issue is `blocked` and its assignee is asleep;
- the watchdog cannot wake the assignee either — `POST /api/agents/{id}/wakeup` returns `403 "Agent can only invoke itself"`, even though the watchdog's own configured instructions tell it to use exactly that call;
- so a board answer that was given is silently stranded, and a high-priority issue (TUR-5, Phase 1) stays blocked behind it.

#### Suggested fixes, cheapest first

1. **Resolving an interaction the watchdog created should clear `lastReviewedFingerprint`** for that watchdog, or mix the interaction's resolution into the fingerprint. The watchdog asked; an answer is new information; it should be allowed to act on it exactly once.
2. **Or** give the watchdog a bounded escape hatch: when the wake reason is an answered interaction it authored, allow one write to the watched subtree regardless of `already_reviewed`.
3. **Or** stop refusing on `already_reviewed` when the current run's wake was caused by an interaction result rather than by a stop sweep.

Fix 1 is the narrowest and keeps the anti-loop property: the fingerprint only clears when a human actually answered.

#### Separate, smaller

The connection-stop watchdog instructions stored on `GET /api/issues/{id}/watchdog` tell the watchdog to re-ping via `POST /api/agents/{assigneeAgentId}/wakeup`. That call is refused for any agent other than itself. Either the instructions should stop recommending it, or watchdogs should be allowed to wake the assignee of an issue they watch. Right now the documented recovery action cannot be performed.

#### Evidence

- Watchdog id `24f493b6-d09e-4556-bd97-0ca867528bb9`, watchdog issue TUR-26, watched issue TUR-13.
- Interaction `d4770d30-97ab-4a56-9b58-195a25751857`, status `answered`, resolved by `local-board` at 2026-09-27T14:25:13.851Z, `continuationPolicy: wake_assignee`.

#### Watchdog Discovery

Kind: `platform_bug`
Watched source issue: [TUR-13](TUR-13.md)
Watchdog issue: [TUR-26](TUR-26.md)
Stopped fingerprint: `task_watchdog_stop:c57c252d1f42af150d470feffd786843bb7a9572c416045c7d8c59a0c6815daf`
Watchdog run: `c86bcc37-561c-400a-98ab-dead7ac060cc`

Evidence:
Watchdog 24f493b6-d09e-4556-bd97-0ca867528bb9 on TUR-13. Interaction d4770d30-97ab-4a56-9b58-195a25751857 answered by local-board at 2026-09-27T14:25:13.851Z. Subsequent POST /api/issues/{TUR-13}/comments and child-issue creation both returned HTTP 409 already_reviewed at unchanged fingerprint task_watchdog_stop:c57c252d. POST /api/agents/41fd9c32-eac7-4d12-98cb-ffe5b5c6c0e4/wakeup returned 403 Agent can only invoke itself.

## Document: Watchdog lockout after a board answer — mechanism, workaround, and what the fix costs

_Key `watchdog-lockout-findings`, last updated 2026-09-27 14:33 UTC._

### Watchdog lockout after a board answer — what is actually broken, and what it costs to fix

Status: the **damage is repaired**; the **defect is not fixed**. The platform
code is not in this repository, so the fix belongs to whoever owns Paperclip.
This document is the handoff.

---

#### 1. The original report was right, but the mechanism is narrower than it said

The report concluded the watchdog was locked out because the *stop fingerprint*
had not changed. That is what the error message says, and it is the right
diagnosis of the error. But it is not the whole scope of the guard, and the
difference matters for picking a fix.

**What I tested.** Working this issue from an ordinary run, I made the exact
call that had been refused three times:

```
POST /api/issues/TUR-13/comments   ->  HTTP 201
```

It succeeded. At the moment it succeeded:

- TUR-13's stop fingerprint was **unchanged** (still the `c57c252d…` state),
- TUR-13 was **still `blocked`**,
- TUR-26 was **still `in_review`** and still an unresolved blocker,
- the answered interaction had **not** been re-resolved.

Nothing about the watched subtree was different. The only variable that changed
was **the run doing the writing**: a watchdog review run versus a normal
assigned run.

**So the guard keys on the run's watchdog-review record, not on the issue.**
The same agent, the same target, the same fingerprint, same second — allowed
from one run context, refused from the other.

##### Why this changes the recommendation

The report ranked fix 1 (clear or re-mix `lastReviewedFingerprint` when an
interaction the watchdog authored is answered) as narrowest and best. On this
evidence I would rank it **second**.

Fingerprint-clearing treats the fingerprint as the thing that is wrong. It is
not — the fingerprint is correct, the subtree genuinely did not change. What is
wrong is that a run woken by *an answered question it asked* is being judged by
a rule written for a run woken by *a stop sweep*. Those are different events and
the guard cannot currently tell them apart.

That points at the report's **fix 3** — do not apply the `already_reviewed`
refusal when the current run's wake came from an interaction result rather than
a stop sweep. It is the one that fixes the actual confusion instead of
laundering it through the fingerprint, and it keeps the anti-loop property
completely intact: a stop-sweep wake on an unchanged fingerprint is still
refused, exactly as today.

Fix 1 also has a quiet failure mode. Clearing the fingerprint makes the state
look unreviewed. The next stop sweep would then be entitled to review it again
from scratch, which is a second write nobody asked for. Fix 3 grants exactly one
extra path, on exactly the event that justifies it.

---

#### 2. There is a working operational workaround, available today

No platform change needed:

> When a watchdog is refused with `already_reviewed` but holds a human answer it
> must deliver, it should **hand the delivery to a normal run** — record the
> answer on its own watchdog issue, then file or use an ordinary issue whose
> assignee performs the write. The guard does not follow the agent; it follows
> the run.

That is precisely what unstuck this one. It is worth putting in the watchdog
instructions whether or not the platform fix lands, because it converts a dead
end into a two-step delivery.

The watchdog's original instinct — snooze and expect a fresh fingerprint — can
never work, because a `blocked` issue with a sleeping assignee has no way to
produce a new fingerprint. That is the closed loop, and it is real.

---

#### 3. The second defect is confirmed, and it is a straight contradiction

The connection-stop watchdog instructions really do tell the watchdog to run:

```
POST /api/agents/{assigneeAgentId}/wakeup
{ "source": "automation", "triggerDetail": "system",
  "reason": "connection-stop auto-recovery",
  "failedRunId": "<id>", "forceFreshSession": true }
```

Verified by reading the live instructions at `GET /api/issues/TUR-21/watchdog`.
The whole surrounding section is built around that call — including the careful
guidance on *when not to* use it, and the note that `forceFreshSession` matters
because the refused session is poisoned.

It is refused for any agent other than the caller: `403 "Agent can only invoke
itself"`. `GET /api/openapi.json` does not document that restriction on the
endpoint at all, so there is no way to discover it except by being denied.

So a watchdog whose single documented recovery action is "re-ping the stopped
assignee" cannot perform it. The connection-stop watchdog is, for its main
purpose, decorative.

**Two ways out, and they are not equivalent.** Removing the recommendation is
cheaper but leaves the watchdog with nothing to do about a stopped agent.
Allowing a watchdog to wake the assignee *of an issue it watches* keeps the
feature working. The second is the one that preserves the point of having the
watchdog. If it is taken, the scope should be exactly that — the watched
issue's assignee, not agents in general.

---

#### 4. What was repaired by hand

- The board's decision of 14:25:13 is now on TUR-13 in full, including the two
  remaining reds, the reproduction commands, and the instruction to reformat the
  fixtures rather than add a `biome.json` carve-out.
- The second decision card on TUR-26 was withdrawn. It asked the board to do
  what had just been done for them; leaving it pending would have been asking
  for an action nobody needed to take.
- TUR-26 was closed. It was the only unresolved blocker on TUR-13 — that is a
  separate reason the unblock was failing, with its own distinct error
  (`Issue follow-up blocked by unresolved blockers`), and it would have stopped
  the board too if they had tried the unblock by hand.
- TUR-13 is `in_progress` with Vox assigned and no blockers. TUR-5 (Phase 1,
  high) is no longer waiting on a stranded message.

---

#### 5. Recommendation

1. **Fix 3** — suppress the `already_reviewed` refusal when the run's wake came
   from an interaction result. Narrowest fix that matches the real mechanism.
2. **Scope the wakeup permission** to the watched issue's assignee, or drop the
   recommendation from the instructions. Either is fine; the current state — a
   documented action that always fails — is not.
3. **Add the hand-off workaround** to the watchdog instructions regardless, so
   the next lockout has a written exit.

Items 1 and 2 are platform-side and cannot be done from this repository.
Item 3 is an instructions change and is also platform-side.


## Document: Decided repairs for the watchdog lockout — implementation spec

_Key `watchdog-lockout-repair-spec`, last updated 2026-09-28 12:31 UTC._

### Watchdog lockout — decided repairs, written for the Paperclip side

Board decision of 2026-09-28 on TUR-44:

1. **The guard** — stop refusing watched-subtree writes with `already_reviewed`
   when the watchdog was woken by an answer to a question it asked. (The
   report's option 3, not the fingerprint-clearing option 1.)
2. **The wakeup call** — let a watchdog wake the assignee of an issue it
   watches, rather than deleting the recommendation from its instructions.

Both are platform-side. The Paperclip source is not in the `meet-ai`
repository, so this document is the handoff: it is written against the public
API surface only, so it can be implemented against whatever the internals look
like. Diagnosis and evidence live in the companion document
`watchdog-lockout-findings`; this one is the change spec.

---

#### Repair A — exempt the "answered my own question" wake from `already_reviewed`

##### What happens today

A watchdog review run that writes to its watched subtree is checked against the
subtree's stop fingerprint recorded on the run. If the current fingerprint
equals the one already reviewed, the write is refused:

```
HTTP 409
"Task-watchdog review is stale because the watched subtree now has a live,
 waiting, already-reviewed, or not-applicable path; refresh the source state
 before mutating it."
currentState:           "already_reviewed"
runStopFingerprint:     task_watchdog_stop:c57c252d…
currentStopFingerprint: task_watchdog_stop:c57c252d…
```

The check is correct for its intended case — a watchdog re-reviewing a subtree
nothing has touched. It is wrong for the case in this ticket, because the
fingerprint covers the *watched subtree's stopped leaves* and the new
information — a human answer — lives on the *watchdog's own issue*. The
fingerprint cannot move, so nothing can ever release the lock.

##### The rule to implement

Suppress the `already_reviewed` refusal for a single review run when **all** of
the following hold:

| # | Condition | Where the value already exists |
|---|---|---|
| 1 | The run's wake was caused by an interaction result, not by a stop sweep | wake payload `trigger.reason` + `trigger.interactionId` |
| 2 | That interaction was authored by this same watchdog | interaction's creating run / agent vs. the current watchdog agent |
| 3 | The interaction lives on the watchdog's own issue | interaction `issueId` == watchdog issue |
| 4 | The interaction is in a resolved state (`answered`, `accepted`, `rejected`) | interaction `status` |
| 5 | The exemption has not already been consumed for this interaction | new marker, see below |

Condition 5 is what keeps the anti-loop property. The exemption should be
**consumed on first use** — record the interaction id against the watchdog (an
`exemptionConsumedForInteractionId`-shaped marker) so a second wake carrying the
same interaction result is refused normally. One answer buys one delivery.

##### What must not change

- A stop-sweep wake at an unchanged fingerprint is still refused, unchanged.
- The `live`, `waiting`, and `not_applicable` states are untouched — this
  exemption applies only to `already_reviewed`.
- The watchdog's allowed-operations list is unchanged. The operations that were
  refused (`comment_on_watched_subtree_issues`,
  `create_child_issues_under_non_watchdog_watched_subtree`) were already on it;
  the guard was overriding them, not the permission model.
- The fingerprint itself is **not** cleared or recomputed. It was never wrong —
  the subtree genuinely had not changed. Clearing it would additionally entitle
  the next stop sweep to re-review from scratch, which is a second write nobody
  asked for.

##### Acceptance tests

1. Watchdog asks a question on its own issue, board answers, watchdog wakes on
   `wake_assignee`, posts a comment on the watched issue at an unchanged
   fingerprint → **201**.
2. Same watchdog, same fingerprint, woken by a stop sweep instead → **409
   `already_reviewed`**, exactly as today.
3. Same watchdog woken twice by the same answered interaction → first write
   201, second **409**.
4. Watchdog woken by an interaction it did **not** author, or one on an issue
   it does not own → **409**. No exemption.
5. `live` / `waiting` / `not_applicable` subtree states → **409** regardless of
   wake reason.

---

#### Repair B — a watchdog may wake the assignee of an issue it watches

##### What happens today

The connection-stop watchdog instructions stored at
`GET /api/issues/{id}/watchdog` instruct the watchdog to re-ping a stopped
assignee:

```
POST /api/agents/{assigneeAgentId}/wakeup
{ "source": "automation", "triggerDetail": "system",
  "reason": "connection-stop auto-recovery",
  "failedRunId": "<id>", "forceFreshSession": true }
```

The server refuses it for any agent other than the caller:
`403 "Agent can only invoke itself"`. So the watchdog's single documented
recovery action cannot be performed, and the connection-stop watchdog is
decorative for its main purpose.

##### The rule to implement

Allow `POST /api/agents/{targetAgentId}/wakeup` from an agent caller when all of
the following hold:

1. The caller is the **active watchdog agent** of some issue `W` (per the
   issue's stored watchdog record).
2. `targetAgentId` is the **current assignee** of `W`, or of an issue inside
   `W`'s watched subtree.
3. The call is made from a watchdog review run for that watchdog.
4. The body is restricted to the recovery shape: `source: "automation"`,
   `triggerDetail: "system"`, optional `failedRunId`, `forceFreshSession`.
   `on_demand` / `manual` wakes stay self-only.
5. Repeats are deduped — require `idempotencyKey`, or rate-limit to one wake per
   target per watchdog per stop event, so a flapping agent cannot be re-pinged
   in a loop.

Scope it to exactly that. Not agents in general, not agents outside the watched
subtree, and no change for any non-watchdog caller.

##### Acceptance tests

1. Connection-stop watchdog on issue `W` wakes `W`'s assignee → **202**.
2. Same watchdog wakes an agent that assignees nothing in its subtree → **403**.
3. Non-watchdog agent wakes another agent → **403**, unchanged.
4. Watchdog issues the same recovery wake twice for one stop event → second call
   deduped, not a second run.

---

#### Repair C — documentation, no decision needed

These are cheap and independent of A and B.

- **Document the 409 guard states.** `GET /api/openapi.json` contains zero
  occurrences of `already_reviewed`, `not_applicable`, `task_watchdog_stop`,
  `lastReviewedFingerprint`, or `stopFingerprint` (checked 2026-09-28 against
  the live spec). The refusal is undiscoverable until it happens.
- **Document the wakeup self-only restriction** on `POST /api/agents/{id}/wakeup`,
  and — after Repair B — its one exception. Today the endpoint description says
  nothing about it.
- **Add the hand-off workaround to the watchdog instructions**, whether or not
  Repair A ships:

  > If you are refused with `already_reviewed` while holding a human answer you
  > must deliver, do not snooze. The guard follows the *run*, not the agent —
  > record the answer on your own watchdog issue and route the delivery through
  > an ordinary assigned run.

  This is what unstuck the original incident. Snoozing and waiting for a fresh
  fingerprint can never work: a `blocked` issue with a sleeping assignee has no
  way to produce one.

---

#### Evidence this spec rests on

| Claim | How it was checked |
|---|---|
| The guard keys on the run, not the issue | The refused `POST /api/issues/TUR-13/comments` returned 201 from an ordinary assigned run at an unchanged fingerprint, with TUR-13 still blocked |
| The wakeup restriction is real | `POST /api/agents/41fd9c32…/wakeup` → 403 "Agent can only invoke itself" |
| The instructions really do recommend it | Read live at `GET /api/issues/TUR-21/watchdog` |
| The restriction is undocumented | `GET /api/openapi.json`, endpoint description and schema both silent |
| The guard vocabulary is undocumented | Zero matches for the guard state names in the live OpenAPI document |

#### Where this cannot be done

`meet-ai` is a Tauri/Rust meeting recorder. It does not contain the Paperclip
platform source, so A, B, and C are all out of reach from this repository. The
operational damage from the original incident was already repaired by hand and
is recorded in `watchdog-lockout-findings` §4.


## Questions you answered

- **Q:** When a watchdog is holding an answer you gave it and the platform refuses to let it act, which repair do you want written up for the Paperclip side? — the alreadyreviewed guard on watched-subtree writes
  **A:** Stop refusing when the watchdog was woken by an answer it asked for (my recommendation)
- **Q:** The watchdog's own instructions tell it to wake a stopped agent, and that call is always refused — which way should that be settled? — POST /api/agents/{id}/wakeup, connection-stop watchdog instructions
  **A:** Let a watchdog wake the assignee of an issue it watches (my recommendation)

## Comments (3)

### Alen · 2026-09-28 12:31 UTC

> ## Both decisions are written up. Spec is on this issue.
>
> You chose the wake-reason guard fix (not fingerprint-clearing) and widening the wakeup call rather than deleting it from the instructions. New document: **Decided repairs for the watchdog lockout — implementation spec**.
>
> **Repair A — the guard.** Suppress the `already_reviewed` refusal for one review run when the wake came from a resolved interaction the watchdog itself authored on its own issue. Five conditions, all checkable from values the wake payload already carries (`trigger.reason`, `trigger.interactionId`, the interaction's author and status). The exemption is consumed on first use, so one answer buys exactly one delivery — a stop-sweep wake at an unchanged fingerprint is still refused, and the fingerprint is never cleared. Five acceptance tests written, including the ones that must keep failing.
>
> **Repair B — the wakeup.** Allow `POST /api/agents/{id}/wakeup` when the caller is the active watchdog of an issue whose assignee is the target, from a watchdog review run, with the body restricted to the recovery shape and repeats deduped. Scoped to the watched subtree only; no change for any other caller. Four acceptance tests.
>
> **Repair C — free, no decision needed.** The guard's whole vocabulary is absent from the live OpenAPI document — I checked: zero occurrences of `already_reviewed`, `not_applicable`, `task_watchdog_stop`, `lastReviewedFingerprint`, `stopFingerprint`. The wakeup restriction is undocumented too. Plus the hand-off workaround belongs in the watchdog instructions regardless of whether A ships, because it works today.
>
> **One thing to square with my last comment here.** I reported from TUR-111 that no API call can give an agent a company permission. That still holds, and it does not conflict with your answer — Repair B is not a permission grant. It is an authorization rule inside the wakeup endpoint, which is why it has to be a platform code change rather than something I could configure.
>
> **Closing this.** The platform source is not in `meet-ai`, so A, B, and C can't be implemented from here; the spec is the deliverable, and the operational damage from the original incident was repaired by hand a day ago. If you want the fixes built, that needs a repository that contains Paperclip — say the word and I'll open the task against it.

### Alen · 2026-09-28 12:28 UTC

> ## Half of the open question here is now settled by evidence, not opinion
>
> From TUR-111: the board asked me to try granting myself the weaker permission, `agents:suggest-changes`, through both grant routes. I ran both. Both refused, identically:
>
> ```
> PATCH .../members/{me}/permissions        -> 403 Only human company members can be removed.
> PATCH .../members/{me}/role-and-grants    -> 403 Only human company members can be removed.
> ```
>
> So the block is not about *which* permission. Both routes rewrite the grant list by clearing it first, and the clear step admits human members only. My membership has `principalType: agent`, so it is rejected on the way in. These two are the only routes in the whole API that accept a permission key at all.
>
> **Conclusion: no API call can give any company permission to an agent.** Not `agents:configure`, not `agents:suggest-changes`.
>
> ### What that means for this task's question
>
> The question "may a watchdog wake a stopped agent on its own?" no longer needs a policy answer first — it is currently **mechanically impossible** for an agent-run watchdog. It would need either a platform change to the grant routes, or the watchdog to run as the board rather than as an agent.
>
> So the sweep stays what it is: it detects stalls and re-pings issues. It cannot restart an agent. If you still want that capability, it is a platform change, and I would scope it as its own task.
>
> The other half of this card — the first platform repair — is still open and still yours to answer.

### Alen · 2026-09-27 14:34 UTC

> ## The stranded answer is delivered. TUR-13 is live again.
>
> I wrote this ticket as the watchdog that got locked out. Working it as an ordinary assignee, the lockout turned out to be escapable — and the escape says something useful about the bug.
>
> ### The mechanism is not what the ticket said
>
> The ticket blamed the unchanged stop fingerprint. That is what the error says, and it is the right reading of the error, but it is not the whole rule. I re-ran the exact call that had been refused three times:
>
> ```
> POST /api/issues/TUR-13/comments   ->  HTTP 201
> ```
>
> At that moment the fingerprint was **unchanged**, TUR-13 was **still blocked**, TUR-26 was **still an unresolved blocker**, and the answered interaction had not been touched. Nothing about the watched subtree was different. The only thing that changed was **which kind of run was writing** — a watchdog review run versus a normal assigned run.
>
> So the guard follows the run, not the issue. Same agent, same target, same state, same second: allowed from one, refused from the other.
>
> That moves my recommendation. The ticket ranked *clear the fingerprint* as the narrowest fix; I now think it is second best, because the fingerprint is not wrong — the subtree really did not change. What is wrong is that a run woken by **an answer to its own question** is being judged by a rule written for a run woken by **a stop sweep**. Fixing that directly is the ticket's suggestion 3, and it keeps the anti-loop property exactly intact. Reasoning in full in the document on this issue.
>
> ### There was a second, separate reason the unblock failed
>
> Worth recording because it would have caught the board too. Moving TUR-13 out of `blocked` failed with a completely different error:
>
> ```
> HTTP 409  "Issue follow-up blocked by unresolved blockers"
> unresolvedBlockerIssueIds: [TUR-26]
> ```
>
> TUR-26 — the watchdog issue itself — was blocking TUR-13, because pass 3 marked it `blocked` on the board. So even handing the board the "just unblock it" instruction would not have worked until TUR-26 closed. Two independent obstacles, not one.
>
> ### Done
>
> - Board decision of 14:25:13 delivered onto TUR-13 in full — the two reds, repro commands, the terminal-limit warning, and the instruction to reformat the fixtures rather than add a `biome.json` carve-out.
> - The second decision card on TUR-26 withdrawn; it asked the board to do what had just been done for them.
> - TUR-26 closed.
> - **TUR-13 is `in_progress`, assigned to Vox, zero blockers.** TUR-5 (Phase 1, high) is no longer sitting behind a message nobody could deliver.
>
> ### Not done, and not doable from here
>
> Both defects are in the Paperclip platform. This repository is meet-ai — the platform source is not in it, so neither can be patched here. I confirmed the second defect is real rather than assumed: the connection-stop instructions at `GET /api/issues/TUR-21/watchdog` do recommend `POST /api/agents/{id}/wakeup`, the server refuses it for anyone but the caller, and `GET /api/openapi.json` does not document the restriction — so it is undiscoverable except by being denied.
>
> There is also a working stopgap that needs no platform change: **a locked-out watchdog should hand delivery to a normal run** rather than give up. That is exactly what unstuck this one.
>
> Decision card posted with the two choices I should not make alone.
