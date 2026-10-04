# Manual checks: TUR-92 (plain wording, no em dashes)

Ticket: https://linear.app/meet-ai-app/issue/TUR-92/plain-natural-wording-across-the-whole-ui-no-em-dashes

Copy-only change: string literals and JSX text. The vitest suite renders every
screen whose text changed, and the tests that check text now check the new
wording. What could not run here is the app itself, so nobody has seen the new
text at real window widths.

## Checks to run

| # | What to do | Expected | Why skipped here |
|---|---|---|---|
| 1 | Signed build. Go through onboarding (permission, folder, agent steps). | No em dash anywhere. Text fits; no sentence wraps badly at the default window width. | Needs the running app. |
| 2 | With no meetings yet, open the meeting list. | The empty state reads "Press ⌘⇧R from anywhere, even with this window behind Zoom, and meet-ai starts recording…". | Needs the running app. |
| 3 | Record a short meeting and watch the live transcript hint. | "Transcribing on this Mac with Apple Speech. Finished lines are saved to transcript.md". | Needs a mic, the system-audio permission and the app. |
| 4 | Settings > tracker, with an agent that has servers. | Each option reads "name (Connected)" and similar. | Needs a signed-in agent CLI. |
| 5 | Start a folder move while a recording is starting. | The error reads "meet-ai is still writing to your meetings folder: a recording starting or stopping…". | Needs the app and a recording. |

## Choices made

- En dashes in ranges: kept. The only one in the UI is the time range on the
  Today pane ("9:00 – 9:30"), which is the usual way to write a time range.
- `formatBytes` shows "unknown size" instead of "—" for a size it cannot read.
- R11 checks `src/` and `src-tauri/src/`. `crates/` is not checked (mostly logs
  and CLI output); its three strings that reach the window were reworded by hand.
