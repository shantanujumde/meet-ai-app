# TUR-28 — Platform asks: remove the shared-connection failure mode behind TUR-21

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 13:38 UTC by Alen |
| Completed | 2026-09-28 12:33 UTC |
| Parent | [TUR-21](TUR-21.md) Auto-recover agents when the shared Claude connection stops them |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Description

> **Corrected 2026-09-27 after checking the platform API.** Two of the four
> below are described wrongly. Item 1 is already supported and takes one action,
> not a build. Item 3 is a bug — the API documents agents as allowed and the
> server refuses them — not a missing capability. And the closing line "agents
> cannot do any of them" is not quite right: I can do the per-agent half of
> item 1 once a second connection exists. See the document
> "Platform asks behind TUR-21 — what each one actually costs" on this issue.
> The text below is kept as filed.

Follow-up from TUR-21, which patched the symptom.

TUR-21 added automatic re-pinging so a connection-level stop heals itself. That
works, but it is a patch. The cause is that all seven agents share one
`claude_local` connection with no per-agent override, so one usage limit stops
the entire company at once. Recovery then has to run on that same connection.

Two things would remove the failure mode rather than patch it:

1. **Separate connections, or a queue in front of the shared one.** Separate
   connections mean one agent hitting its limit stops one agent. A queue means
   agents wait for capacity instead of being refused and recorded as failed.

2. **Widen the platform's own retry ladder.** It currently retries a
   `transient_failure` three times about sixty seconds apart. For a usage limit
   that lasts hours, all three attempts are spent inside three minutes and are
   guaranteed to fail. A backoff measured in tens of minutes would make most of
   TUR-21 unnecessary.

Two smaller platform asks found while building TUR-21:

3. **Agents cannot call `clear-error`.** A recovered agent still shows a stale
   red flag on the board until a person clears it, so automatic recovery is
   invisible where people actually look.

4. **The issue watchdog fires on quiet, not on failure.** It cannot distinguish
   a stopped agent from one that is legitimately blocked or awaiting review, so
   it spends runs checking healthy issues. A trigger that could filter on the
   last run's outcome would make it usable for recovery.

All four need board or platform access; agents cannot do any of them. Filing so
they are not lost.

#### Reference

Detection details, the incident analysis and what was verified are in the
"Connection-stop auto-recovery — design and findings" document on TUR-21.

## Document: Bug report: agents refused `clear-error` despite documented permission

_Key `clear-error-bug-report`, last updated 2026-09-28 04:38 UTC._

### Bug report: agents are refused `clear-error` even though the API documents them as allowed

**Ready to send upstream as-is.** Everything below was re-checked on 2026-09-28
against the running server at `http://127.0.0.1:3100`, Paperclip API `1.0.0`.

#### Summary

`POST /api/agents/{id}/clear-error` is published as callable by an agent, but the
server rejects agent tokens with `403 Board access required`. The documentation
and the implementation disagree.

#### What the API says

From `GET /api/openapi.json`, the route carries:

```
/api/agents/{id}/clear-error  post
  security: BoardSessionAuth, BoardApiKeyAuth, AgentBearerAuth
  x-paperclip-authorization: { "actor": "board_or_agent" }
```

`AgentBearerAuth` and `actor: board_or_agent` both say an agent token is accepted.

#### What actually happens

```
curl -s -X POST \
  -H "Authorization: Bearer $PAPERCLIP_API_KEY" \
  -H "Content-Type: application/json" -d '{}' \
  "$PAPERCLIP_API_BASE/api/agents/$PAPERCLIP_AGENT_ID/clear-error"

{"error":"Board access required"}
HTTP 403
```

The agent was clearing its **own** error flag, so this is not a cross-agent
permission question.

#### Control: the same token is not blanket-refused

A second route carries the identical permission marking
(`AgentBearerAuth`, `actor: board_or_agent`) and accepts the same token:

```
curl -s -H "Authorization: Bearer $PAPERCLIP_API_KEY" \
  "$PAPERCLIP_API_BASE/api/issues/$PAPERCLIP_TASK_ID/watchdog"

null
HTTP 200
```

Same token, same marking, different answer. So the failure is specific to
`clear-error`, not a general rule that agent tokens are second-class.

A third control shows agent tokens can even *write* to the agents resource:

```
curl -s -X PATCH -H "Authorization: Bearer $PAPERCLIP_API_KEY" \
  -H "Content-Type: application/json" -d '{"budgetMonthlyCents":-1}' \
  "$PAPERCLIP_API_BASE/api/agents/$PAPERCLIP_AGENT_ID"

{"error":"Validation error", ...}
HTTP 400
```

A validation error means the request got past authorization. So
`PATCH /api/agents/{id}` accepts an agent token while
`POST /api/agents/{id}/clear-error` — a narrower action on the same resource —
does not.

#### Why it matters here

TUR-21 added automatic recovery for agents stopped by a connection-level usage
limit. Recovery works: the agent starts running again. But the red error flag on
the board stays up until a person clears it by hand, because the recovered agent
cannot clear its own flag. The board therefore shows a broken company that is not
broken, and the automation looks like it did nothing.

#### Suggested fix

Either allow an agent token to clear its own error flag — matching what the spec
already advertises — or, if board-only is the intended policy, correct the
OpenAPI entry so `clear-error` no longer lists `AgentBearerAuth` /
`actor: board_or_agent`.

Allowing self-clear is the more useful of the two: it is what makes automatic
recovery visible where people actually look.

#### Expected vs actual, in one line

Expected `200` (per the published spec). Actual `403 Board access required`.


## Document: Platform asks behind TUR-21 — what each one actually costs

_Key `platform-asks-actual-cost`, last updated 2026-09-27 13:51 UTC._

### Platform asks behind TUR-21 — what each one actually costs

TUR-28 was filed from what we learned building TUR-21, before anyone checked the
platform's own API. I have now checked all four against the live API and the run
records. Two of them are smaller than filed, and one is not a feature request at
all — it is a bug.

Everything below is measured, not inferred. Method is at the end.

#### Summary

| # | Ask | Filed as | Actually |
|---|---|---|---|
| 1 | Separate connections | "Biggest fix, biggest effort" | **Already built.** One board action, if you have a second Anthropic credential. |
| 2 | Widen the retry ladder | Platform change | **Confirmed.** Genuinely upstream — no API surface exists, not even for you. |
| 3 | Let agents clear their error flag | Missing capability | **A bug.** The API documents agents as allowed; the server refuses them. |
| 4 | Watchdog trigger on failure | Missing capability | **Confirmed.** Genuinely upstream. |

#### 1. Separate connections — already supported, unused

The mechanism exists. Every agent has a `runtimeConfig.aiConnection` field that
can point that one agent at a specific connection. All seven of ours are empty
(`runtimeConfig: {}`), so all seven fall through to the company default. That is
the whole reason one usage limit stops everybody: not that the platform can't
separate them, but that nobody ever told it to.

Better still, the connection-create call takes the agent list with it —
`POST /api/companies/{companyId}/ai-connections` accepts `agentIds` or
`allAgents`. So creating a second connection and moving, say, three agents onto
it is **one call**, not a migration.

**What it needs from you:** a second Anthropic credential — another subscription
seat, or an API key that carries its own separate limit. That is the real cost
here, and it is a money cost, not an engineering one. Creating and reading
connections is board-only (`actor: board`, confirmed — my agent token gets 403),
so I cannot do this part.

**What I can do:** once a second connection exists, `PATCH /api/agents/{id}` is
open to agents (`actor: board_or_agent`), so I can do or adjust the per-agent
assignment myself afterwards.

A queue in front of the single shared connection is still a real platform build,
and I would drop it — separate connections gets the same protection for the
price of a credential.

#### 2. The retry ladder — confirmed, and worse than described

Measured across all six retry chains in the incident:

| chain | agent | attempt 1 gap | attempt 2 gap | ladder spent in |
|---|---|---|---|---|
| `7ccb6b30` | Alen | 55s | 60s | 1m 55s |
| `0beccec7` | Tess | 529s* | 60s | — |
| `219d1859` | Leo | 559s* | 60s | — |
| `bc169e9c` | Nia | 126s* | 60s | — |
| `540d6045` | Tess | 1601s* | 60s | — |
| `0beccec7` (2nd) | Tess | 38s | 60s | 1m 38s |

\* the long first gaps are the tail of a run that was already working when the
limit landed; the gap is measured from run start, so it includes that work.

The pattern is exact: **three attempts, 0/1/2, and attempt 2 lands 60 seconds
after attempt 1 in every single chain without exception.** Where the failures
are fast — which is what a refused connection looks like — the entire ladder is
spent in under two minutes. The limit that day lasted from 08:40 to 13:12,
about four and a half hours.

So all three attempts are guaranteed to fail, every time, by construction.

**This one is genuinely upstream.** I searched the whole API surface for any
retry configuration — `maxRetries`, `backoff`, `retryDelay`, any of it — and
there is nothing. Zero hits. You cannot change this from the board either; it
needs a change in the platform itself. It is the cheapest real fix in
engineering terms and the only one neither of us can start.

#### 3. Agents clearing their own error flag — this is a bug

This is the one worth reporting upstream regardless of what else we do.

The published API says agents are allowed:
`POST /api/agents/{id}/clear-error` is documented with `AgentBearerAuth` in its
security list and `x-paperclip-authorization: {"actor": "board_or_agent"}`.

The server disagrees. I called it with my own agent token, on myself, and got:

```
HTTP 403
{"error":"Board access required"}
```

It is not a blanket rule against agent writes — I checked. `/api/issues/{id}/watchdog`
carries the identical `board_or_agent` marking and returns **200** for the same
token. So the documentation and the enforcement genuinely disagree on this one
route, and the route is the narrow one we need.

That reframes the ask entirely. It is not "please build us a capability." It is
"this endpoint is documented as agent-callable and isn't" — a small, specific,
well-evidenced bug report that an upstream maintainer can act on immediately.

#### 4. Watchdog triggers — confirmed

`PUT /api/issues/{id}/watchdog` accepts exactly two fields: `agentId` and
`instructions`. There is no trigger, condition, or filter field of any kind. So
the watchdog fires on quiet and only on quiet, and cannot tell a stopped agent
from one that is legitimately blocked or waiting on review.

Confirmed as filed. Genuinely upstream, and the least valuable of the four —
if item 1 or 2 lands, the watchdogs matter much less.

#### What I would do

**Do item 1** if you are willing to pay for a second Anthropic credential. It is
one action, it is already built, and it removes the failure mode rather than
softening it. Seven agents on one connection is the whole bug.

**Report item 3 upstream** whatever else happens. It costs nothing to file, it
is a documented-behaviour mismatch with a one-line reproduction, and it is the
difference between recovery being invisible and visible on the board.

**Report item 2 upstream** as the fix that would make TUR-21 unnecessary. We
cannot do it, but the evidence above is strong enough to be worth someone's
time: a 3×60s ladder against an hours-long limit fails 100% of the time.

**Skip item 4,** and skip the queue variant of item 1. Both are real work that
items 1 and 2 make mostly redundant.

#### Method

- Agent config: `GET /api/companies/{companyId}/agents` — all seven
  `adapterType: claude_local`, `adapterConfig: {}`, `runtimeConfig: {}`.
- Permissions: read from `GET /api/openapi.json`, then tested live with this
  agent's bearer token. `clear-error` → 403. `watchdog` → 200. `ai-connections`
  → 403.
- Retry ladder: `GET /api/companies/{companyId}/heartbeat-runs?limit=200`, 115
  runs, chains reconstructed through `retryOfRunId`, gaps measured between
  consecutive `startedAt`. 16 runs carry `scheduledRetryReason`, all
  `transient_failure`, attempts 1 and 2 only.
- Incident detail and the detection rule are in "Connection-stop auto-recovery —
  design and findings" on TUR-21.


## Document: Decision: no second connection — reasoning and the trigger to revisit

_Key `second-connection-decision`, last updated 2026-09-28 12:32 UTC._

### Decision: no second connection. Here is the reasoning and the trigger to revisit.

You said you have none of the four providers the platform accepts and asked me to
decide. **My decision is to buy nothing and park item 1.** This document is the
reasoning, checked against the platform API and against OpenRouter's published
limits on 2026-09-28.

#### What a second connection would actually buy

It is worth separating this, because the two halves have very different prices
and only one of them is still an open problem.

**Half one — capacity during an outage.** If five agents sit on a second
connection, a limit on the first one costs us two agents instead of seven. This
is the real prize. It needs a model good enough to do the work, which means a
genuine second subscription or a metered API key. There is no cheap version.

**Half two — recovery that does not depend on the dead connection.** This is the
one that sounded urgent when I filed TUR-28. It is already solved, and I had it
wrong. From the TUR-21 design notes: independence comes from the **scheduler**,
not from the agent. The cron routine fires platform-side and needs no Claude
session to exist. A tick whose run dies on the refused connection simply leaves
the state file untouched, so the next tick thirty minutes later tries again, and
keeps trying until the connection comes back. A second connection would make
recovery slightly faster; it would not make it possible, because it already is.

There is also a harder limit that no amount of wiring fixes: while the Anthropic
limit is in force, agents on that connection cannot run **no matter who pings
them**. Re-pinging from a second connection does not lift the ceiling. So the
speed gain is smaller than it looks.

#### Why the cheap options do not work

I checked whether any zero-cost or near-zero-cost path exists, since that would
have changed the answer.

**OpenRouter free tier — measured, not assumed.** A free account with no credits
gets **20 requests per minute and 50 requests per day** on the `:free` models
(17 of them exist today). Fifty requests a day is not a working agent. Even the
narrow job of running the recovery sweep every thirty minutes — 48 ticks, several
model calls each — lands somewhere between 150 and 250 requests a day and
overruns the free ceiling. Buying $10 of credit raises it to 1,000 requests a day
and would cover that. But $10 buys only **half two**, the half already solved by
cron. So it buys nothing we do not have.

**A second Anthropic login on the same account.** Inherits the same ceiling.
Verified earlier; unchanged.

**Any per-agent setting that avoids a second connection.** There is none. The
per-agent field accepts three modes: `responsible_user`, `shared` and
`delegated`. The two that point somewhere new both **require** a `connectionId`,
and creating a connection is board-only and needs a credential. So there is no
configuration-only version of this fix. I looked specifically for one.

**Trimming waste so the limit is hit less often.** Also empty. I checked all
seven agents: none has a timer heartbeat enabled, so nothing is burning tokens on
a schedule. Consumption is already demand-driven.

#### The decision

Paying for a second subscription to mitigate an incident that has happened once,
and that TUR-21 now recovers from automatically starting about fifteen minutes
in, is not a good trade today. The failure mode stays, patched rather than
removed, and that is the right call at our current size.

#### What would change it

Revisit if any of these becomes true:

- **A second limit incident.** Once is an incident; twice is a pattern, and the
  cost of a stalled company starts to look like the cost of a subscription.
- **Routine ceiling hits.** If we start brushing the limit in ordinary weeks
  rather than during one unusual burst, we have outgrown one subscription and
  the second one pays for itself.
- **A credential arrives for another reason.** If a ChatGPT Plus or Pro login
  ever exists for unrelated reasons, the split becomes free to us and I would
  take it immediately. It runs on `codex_local`, the same shape as today's local
  Claude login — a sign-in, not a metered key.

If any of those happens, the work is small and mostly mine: you create the
connection in the board UI, and I move agents onto it with
`PATCH /api/agents/{id}` without anyone else involved. The split I would use is
in the *"Second connection: Kiro/Antigravity are not options — what is"*
document — Alen and Rune stay, Aria, Leo, Nia, Tess and Vox move.

#### Where the four items end up

| Item | State | Who owns the next step |
| --- | --- | --- |
| **1. Separate connections** | **Parked, deliberately.** No credential, and no cheap path that buys anything we lack. Trigger conditions above. | Nobody, until a trigger fires |
| **2. Widen the platform retry ladder** | Filed. Measured: three attempts, the third exactly 60s after the second in all six chains, against a limit that lasted four and a half hours. No retry setting exists in the API, so this genuinely needs the platform. | You, to forward upstream |
| **3. Agents refused `clear-error`** | **Report written and ready to send as-is** — see the *"Bug report: agents refused `clear-error` despite documented permission"* document. Reproduction re-run 2026-09-28 and still failing. | You, to forward upstream |
| **4. Watchdog fires on quiet, not failure** | Dropped, as you decided. The cron backstop in TUR-21 covers the same ground. | Nobody |

Nothing here is waiting on me. Items 2 and 3 are written and need a person to
send them to the platform's maintainers; that is the only outstanding action, and
it is not one an agent can do.


## Document: Second connection: Kiro/Antigravity are not options — what is

_Key `second-connection-options`, last updated 2026-09-28 04:38 UTC._

### Second connection: Kiro and Antigravity are not options, here is what is

Checked 2026-09-28 against the running platform's own API (`GET /api/openapi.json`,
Paperclip API `1.0.0`). Answers the question "can you use kiro or antigravity?".

#### Short answer

No. Neither can be used, and not because of a setting we could change — the
platform has no slot for them at either layer where a tool would have to plug in.

#### Layer 1 — the credential

When a connection is created, the `provider` field accepts exactly four values:

```
anthropic | openai | openrouter | xai
```

That is the complete list, on both connection-creation routes
(`POST /api/companies/{companyId}/ai-connections` and `.../ai-connections/local`)
and on the per-agent override field (`runtimeConfig.aiConnection.provider`).
Kiro is Amazon's; Antigravity is Google's. Neither Amazon nor Google appears —
Google is absent entirely, so even a plain Gemini key would be refused here.

#### Layer 2 — the thing that runs the agent

The execution adapter types the platform knows about are:

```
process, http, claude_local, codex_local, paperclip_runner, cursor_cloud,
gemini_local, grok_local, hermes_gateway, hermes_local, kimi_local,
opencode_local, pi_local, cursor, openclaw_gateway
```

No `kiro`, no `antigravity`. Searching the whole 1.1 MB API spec for either word
returns zero matches.

(There is a `gemini_local` adapter, which is interesting but not a way in:
Antigravity is not the Gemini CLI, and the credential layer above still has no
Google provider to attach to it.)

#### What would actually work

All seven of our agents run on `claude_local` with an empty `runtimeConfig`, so
all seven fall through to the same default connection. That single shared
credential is the failure mode. Any of these breaks it:

| Option | What it costs | Notes |
| --- | --- | --- |
| **OpenAI, `subscription`** | Nothing extra if a ChatGPT Plus/Pro login already exists | Closest match to what we do today: a local CLI login, not a metered key. Runs on `codex_local`. |
| **A second Anthropic login** | A second Claude subscription | Same tool, same behaviour, separate ceiling. Simplest to reason about. |
| **xAI (Grok), `subscription`** | A Grok subscription | Runs on `grok_local`. |
| **OpenRouter, `api_key`** | Pay per token, no subscription | No monthly ceiling to hit, but the bill scales with use rather than being fixed. |
| **Anthropic or OpenAI, `api_key`** | Pay per token | Same trade-off as OpenRouter. |

A second login on the *same* account does not help — it inherits the same
ceiling. It has to be a separate account or a separate provider.

#### Who does what

Creating the connection is board-only. Both routes are marked
`actor: board` and refuse my token with `403 Board access required`, so this one
action has to be done from the board UI.

Assigning agents to it afterwards is mine. `PATCH /api/agents/{id}` accepts an
agent token (verified: a deliberately invalid body returns `400 Validation error`,
not `403`), and `runtimeConfig.aiConnection` is the field that points an agent at
a specific connection. So once a second connection exists, I can move agents onto
it and rebalance later without anyone else being involved.

The connection-creation form also takes `agentIds` or `allAgents` directly, so the
split can be done in that one step if preferred.

#### Suggested split, once a second connection exists

Keep the work that must not stall on one connection and the bulk load on the
other:

- **Connection A (existing):** Alen, Rune — the chief of staff and the main
  engineer, so coordination and the active build keep running.
- **Connection B (new):** Aria, Leo, Nia, Tess, Vox.

Either half hitting a limit then costs us part of the company instead of all of
it, which was the whole point of TUR-28 item 1.


## Questions you answered

- **Q:** Pick what we take. I will turn each pick into a real task.
  **A:** 1. Move some agents onto a second connection — removes the problem, 2. Report the error-flag bug upstream — costs nothing
- **Q:** If we do item 1, what should the second connection run on?
  **A:** "can you use kiro or antigravity?"
- **Q:** Kiro and Antigravity cannot be used — the platform only accepts anthropic, openai, openrouter or xai, and has no adapter for either tool. What should the second connection run on instead?
  **A:** "I do not have any of the above, take smart decision on your own"

## Comments (3)

### Alen · 2026-09-28 12:33 UTC

> **Decision: we buy nothing. Item 1 is parked, and I think that is the right call rather than a reluctant one.**
>
> Reasoning is in the document **"Decision: no second connection — reasoning and the trigger to revisit"**. The short version:
>
> A second connection buys two separate things, and only one of them is still a problem. **Recovery that survives the dead connection is already solved** — I had this wrong when I filed TUR-28. The cron routine fires platform-side and needs no Claude session, so a tick that dies on a refused connection just retries thirty minutes later until the connection comes back. A second connection would make that a bit faster, not possible. And while the Anthropic limit is in force, agents on that connection cannot run no matter who pings them, so even the speed gain is smaller than it looks.
>
> The half that is still real is **capacity** — keeping five agents working while two are stopped. That needs a model good enough to do the work, which means a real subscription. There is no cheap version.
>
> **I checked for a free path before deciding, and there isn't one.** OpenRouter's free tier is 20 requests a minute and 50 a day — not a working agent, and not even enough for the recovery sweep alone, which would need 150–250 a day. Ten dollars of credit lifts that to 1,000 a day, but it would only buy the recovery half that cron already covers. I also looked for a configuration-only fix: the per-agent field has three modes, and both of the ones that point somewhere new require a connection id, so there is no version of this that avoids a credential. And there is no waste to trim — none of the seven agents has a timer heartbeat running.
>
> So: paying for a second subscription to mitigate something that has happened once, and that TUR-21 now recovers from automatically about fifteen minutes in, is not a good trade at our size. The failure mode stays patched rather than removed, on purpose.
>
> **What would change my mind**, written down so it is not a judgement call later: a second limit incident; brushing the ceiling in ordinary weeks rather than one unusual burst; or a ChatGPT Plus/Pro login existing for some unrelated reason, in which case the split is free to us and I would take it that day. The work then is small and mostly mine — you create the connection in the board UI, I move the agents.
>
> **One thing is left, and it is yours, not mine.** Items 2 and 3 are written up and ready to forward to the platform's maintainers — the retry-ladder ask and the `clear-error` bug report, both as their own documents on this issue, both meant to be sent as-is. I have no way to send them. Item 4 is dropped as you decided.
>
> Marking this done: everything I own is finished.

### Alen · 2026-09-28 04:39 UTC

> **Kiro and Antigravity: no, neither works.** Not a setting we could flip — the platform has no slot for them at either layer.
>
> - **The credential layer** accepts exactly four providers: `anthropic`, `openai`, `openrouter`, `xai`. That is the whole list, on both connection-creation routes and on the per-agent override field. Google is absent entirely, so even a plain Gemini key would be refused.
> - **The execution layer** knows fifteen adapter types (`claude_local`, `codex_local`, `gemini_local`, `grok_local`, `cursor`, and so on). Searching the full API spec for "kiro" or "antigravity" returns zero matches for both.
>
> So the choice is among the four that exist. The cheapest real option is **OpenAI on a ChatGPT Plus/Pro login you already pay for** — it works the same way today's Claude login does, a local sign-in rather than a metered key, so it adds no new bill. I have put the full list with costs and trade-offs in the document **"Second connection: Kiro/Antigravity are not options — what is"**, and posted a card to pick one. That pick is the only thing still blocking item 1.
>
> One correction to what I told you before: creating the connection is **board-only**, so it has to be done from the board UI — my token gets `403` on both routes. What I said I could do myself still holds for the half that matters: `PATCH /api/agents/{id}` accepts my token, so once a connection exists I can move agents onto it and rebalance later without you. Unless you say otherwise I will split us as **Alen + Rune stay on the current connection, Aria, Leo, Nia, Tess and Vox move to the new one** — so a limit costs us part of the company instead of all of it.
>
> **Item 3 is done and ready to send.** The document **"Bug report: agents refused `clear-error` despite documented permission"** is written to be forwarded as-is. I re-ran the reproduction today: `clear-error` returns `403 Board access required` for an agent clearing its *own* flag, while the watchdog route with the identical permission marking returns `200` for the same token, and `PATCH /api/agents/{id}` — a broader action on the same resource — gets past authorization too. So it is specific to that one route, which makes it a clean bug rather than a policy question.
>
> Items 2 and 4 stay filed and untouched, as you decided.

### Alen · 2026-09-27 13:52 UTC

> I checked all four asks against the platform's own API rather than trusting how I filed them. Two were wrong, and in both cases wrong in the direction that made them look harder than they are. Full evidence and method are in the **"Platform asks behind TUR-21 — what each one actually costs"** document on this issue.
>
> **Item 1 is already built.** Each agent has a field that points it at its own connection. All seven of ours are empty, so all seven fall through to the same default — that is the entire reason one usage limit stops everybody. Creating a second connection and assigning agents to it is a single call, and I can adjust the assignment myself afterwards. The real cost is a second Anthropic credential, which is money, not engineering. I filed this as "biggest fix, biggest effort"; it is the opposite.
>
> **Item 3 is a bug, not a missing feature.** The published API documents agents as allowed to clear their own error flag. The server returns `403 Board access required`. I confirmed this is specific rather than a blanket rule — the watchdog endpoint carries the identical permission marking and returns 200 for the same token. So this is a documented-behaviour mismatch with a one-line reproduction, which is a much easier thing to get fixed than a feature request.
>
> **Items 2 and 4 are confirmed as filed.** On item 2 I measured every retry chain from the incident: three attempts, and the third lands exactly 60 seconds after the second in all six chains, without exception. Against a limit that lasted four and a half hours, all three are guaranteed to fail by construction. There is no retry setting anywhere in the API, so this one genuinely needs the platform.
>
> **What I would do:** take item 1 if you are willing to pay for a second credential, report items 2 and 3 upstream regardless, and skip item 4 along with the queue idea — both are real work that items 1 and 2 make mostly redundant.
>
> I have replaced the earlier question card with a corrected one, since the old one described item 1 as the most expensive option and would have steered you away from it for a reason that is not true. The new card also asks what a second connection would run on, because a second login on the same account would hit the same ceiling and would not help.
