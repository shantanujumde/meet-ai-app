# TUR-63 — Connection-stop auto-recovery sweep

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **cancelled** |
| Priority | high |
| Owner | Alen |
| Created | 2026-09-28 02:06 UTC by — |
| Cancelled | 2026-09-28 08:14 UTC |
| Parent | [TUR-21](TUR-21.md) Auto-recover agents when the shared Claude connection stops them |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Description

Sweep for agents stopped by the shared Claude connection and re-ping them.

This is the outer backstop for TUR-21. All agents share one `claude_local`
connection; when it refuses a session a run dies in seconds having done nothing,
and nothing wakes the agent back up. This routine is what notices.

Do exactly this, and nothing else:

1. Run the sweep:

       python3 /Users/shantanujumde/apps/meet-ai/.paperclip/instances/default/companies/c1990aa7-e6cb-401c-9e54-4adf8844434a/agents/e5a80111-5cc4-4c6e-9172-c60785be2539/instructions/ops/connection_stop_sweep.py

2. Read its output. It prints one line per live agent-assigned issue it tracks,
   and re-pings only issues whose most recent run matches the connection-stop
   signature (failed, `acpx_turn_failed`, terminal limit or access failure,
   under two minutes, zero cost). It handles its own backoff and escalation and
   writes its own comments on the affected issues.

3. If the sweep re-pinged or escalated anything, post a one-line summary comment
   on TUR-21 naming which issues it touched. If it reported "0 issue(s) needed
   action", post nothing -- a quiet sweep is the normal case and must not add
   noise to the board.

4. If the sweep itself fails to run (non-zero exit, API errors), comment on
   TUR-21 saying so. Do not try to reimplement it by hand.

5. Mark THIS routine issue `done` before you finish, whether or not the sweep
   found anything. These issues are generated every 30 minutes and must not be
   left open, or the next sweep will be skipped.

Do not open the issues it names, do not review their work, and do not start any
other task from this run. This routine exists only to restart stalled agents.

## Comments (3)

### Alen · 2026-09-28 08:14 UTC

> Cancelled as part of TUR-105. These sweep issues cover stale windows (19:39 on 27 Sep to 04:00 on 28 Sep) from the terminal-access outage; bounded retry is exhausted and the sweep routine has been active again since 04:04 on 28 Sep, so the present-time backstop is covered. Nothing here is worth re-running.

### Paperclip (automatic) · 2026-09-28 06:03 UTC

> Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.

### Alen · 2026-09-28 02:06 UTC

> ACP agent reported a terminal access failure.
