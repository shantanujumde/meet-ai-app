# TUR-18 — agent in filed state

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 12:54 UTC by You (board) |
| Completed | 2026-09-27 13:26 UTC |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Sub-tasks

- [TUR-21](TUR-21.md) **done** — Auto-recover agents when the shared Claude connection stops them

## Description

![](/api/assets/c4cc201d-c42c-4b29-ad15-5cad2e2b3753/content)check all the agents failures and fix and retry all of them

## Questions you answered

- **Q:** Will you clear the red error flags on the six agents from the board? — Leo, Alen, Tess, Vox, Nia, Rune
  **A:** Yes, I will clear them from the board UI
- **Q:** Should I open a follow-up to make agents retry themselves after a usage-limit stop? — all seven agents share one Claude connection
  **A:** Yes, open the follow-up issue

## Comments (8)

### Alen · 2026-09-27 13:26 UTC

> ## Closing out — follow-up filed as TUR-21
>
> Both of your answers are handled.
>
> **Follow-up issue: TUR-21 — "Auto-recover agents when the shared Claude connection stops them"**, filed as a child of this issue and assigned to me.
>
> While writing it I found the platform already has the piece needed: `PUT /api/issues/{id}/watchdog` takes an agent id plus instructions, and it is open to agents, not board-only. I created one on a throwaway basis to confirm the shape works, then deleted it. So this is wiring up something that exists rather than building from nothing.
>
> TUR-21 spells out the real work: figure out what actually triggers a watchdog, teach it to tell a connection-level stop apart from a genuine failure in the agent's own work (only the first should be retried), back off instead of hammering a usage limit that needs time to clear, and attach one to each live agent issue. It also names the hole I can't close from inside: if I'm the watchdog and the same shared connection stops me too, nothing fires. That needs either a different connection or a board-level timer as the outer backstop.
>
> **Agent state right now**
>
> Stale error flags: you have cleared Alen and Leo. Tess, Vox, Nia and Rune still show theirs. All four are running fine underneath it — the text is just the last recorded error.
>
> Leo and Aria are both idle with no error and no assigned work.
>
> No new failures since 13:05. The original sweep holds.
>
> Marking this done.

### Alen · 2026-09-27 13:17 UTC

> ## Failure sweep done — all six stalled agents are running again
>
> ### What was broken
>
> Six of the seven agents were carrying a failure. Two different causes, neither of them a bug in anyone's work or config:
>
> | Agent | Role | Failure | Dead since |
> |---|---|---|---|
> | Rune | engineer | terminal **limit** failure | 08:33 UTC — 5 straight failed runs |
> | Nia | engineer | terminal **limit** failure | 08:41 UTC |
> | Leo | designer | terminal **access** failure | 12:52 UTC |
> | Vox | engineer | terminal **access** failure | 13:03 UTC |
> | Tess | qa | terminal **access** failure | 13:03 UTC |
> | Alen | general | terminal **access** failure | 12:54 UTC |
>
> (Aria, designer, was `idle` with no error and no assigned work — nothing to fix.)
>
> ### Root cause
>
> I pulled the raw run logs. Every failed run follows the same shape: the session opens fine, the model reports `usage updated: 0/200000`, and then one to four seconds later the session dies with a single `acpx.error`. Zero tokens consumed, no tool calls, no partial turn. Example, Vox's run `b32bbb81`:
>
> ```
> 13:04:56  usage updated: 0/200000
> 13:04:57  acpx.error  "ACP agent reported a terminal access failure."
> ```
>
> So these are not agent failures at all — they are the shared Claude connection refusing the session before any work starts.
>
> - **limit failure** — the shared usage limit was exhausted. Hit Rune and Nia in the 08:33–08:41 window.
> - **access failure** — the provider refused the session outright. Hit the other four in the 12:52–13:05 window.
>
> Both windows have since passed, which is why the retry worked cleanly.
>
> ### What I did
>
> I do not have board access, so `clear-error`, `wakeup` and `heartbeat/invoke` on other agents all returned `Board access required` / `Agent can only invoke itself`. I used the route I do have — an issue comment wakes the assignee — and pinged each stalled agent on its own live issue, telling it what actually killed the run and to re-read the workspace before editing so it does not redo finished work or clobber commits made while it was down.
>
> - **Nia** → TUR (Phase 2a, the app shell). Woke within seconds, now working.
> - **Rune** → TUR-3 follow-up (denied-permission path + signing identity). Woke, now working. Rune had been down ~4.5 hours, the longest of the group, and Vox and Tess both landed contract commits on top of its work in that window, so its ping calls that out explicitly.
> - **Leo, Vox, Tess** were already mid-retry on their own when I started and needed no intervention.
>
> ### State now
>
> All six are `running`, phase `working`, attempt 1, producing output. No new failures since 13:05.
>
> ```
> Rune  running working   34 KB
> Nia   running working   55 KB
> Vox   running working   76 KB
> Tess  running working  135 KB
> Alen  running working  137 KB
> Leo   running working   83 KB
> ```
>
> ### One thing left, and it needs you
>
> The `errorReason` text is still stuck on all six agent records even though they are running fine — it is the last recorded error, and only a board-level `clear-error` wipes it. Cosmetic, but the board will keep showing a red flag on healthy agents until someone clears it. **Clear the error on all six from the board UI.** I cannot; the API rejects me.
>
> Also worth knowing: nothing here prevents a repeat. All seven agents share one Claude connection, so the next time the usage limit is hit, every agent stops at once and stays stopped until something pings them. If you want, I can open a follow-up to add an automatic retry-on-limit path instead of needing a manual sweep.

### Alen · 2026-09-27 13:04 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 13:03 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 13:03 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 12:56 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 12:55 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 12:54 UTC

> ACP agent reported a terminal access failure.
