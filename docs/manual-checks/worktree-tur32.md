# Manual checks: TUR-32 (pre-meeting brief: last time's notes and recent commits)

These need the running app with a real meetings folder, so they were not run
here. The headless parts are covered by tests in `src-tauri/src/brief/tests.rs`
(title match with fixture meetings and a temp git repo, today's own meeting
skipped, open tickets only, merges skipped, commits capped, no previous
meeting, no repo / missing folder / not a git folder, `repos.default` used
when the meeting names no repo), `crates/store/tests/index_titled.rs` (the
index lookup by title, any case, newest first) and
`src/routes/Brief.test.tsx` (the view, its empty and error states, the
`/brief?title=` link) and `src/ui/TodayPane.test.tsx` (clicking a meeting
opens its brief; solo blocks are not links).

How it works: `/brief?title=<title>` asks Rust for `meeting_brief`. Rust finds
the newest meeting before today with the same title (trimmed, any case)
through the search index, reads that one folder's `## Summary`,
`## Decisions` and its tickets that are not done or dropped, then runs
`git -C <repo> log --no-merges --since=<that meeting's date> --format='%h %s' -n 20`
in the meeting's `repo`, else `repos.default`.

Not in this wave: matching on a calendar series id (the `Event` type has
none yet; title match only, per the wave defaults), the reminder notification
that opens the brief (TUR-30).

## 1. A recurring meeting shows last week's summary and this week's commits

1. Have a meeting folder from last week titled, say, "Platform Standup", with
   notes written (Summary, Decisions) and one open and one done ticket, and
   `repo: ~/code/<a real repo>` in its `meeting.md` frontmatter. Make a commit
   or two in that repo after last week's meeting time.
2. Put a "Platform Standup" event with at least two attendees on today's
   calendar. Run the app (`just dev` on a dev machine) and click that event
   in the Today pane (or Tab to it and press Enter).
3. Expected: "Before Platform Standup", a "Last time" card with last week's
   summary and decisions, only the open ticket listed, and "Commits since
   then" with this week's commits (short hash and subject, newest first, no
   merge commits, at most 20). "Open last meeting" opens last week's meeting.

## 2. `repos.default` is used when the meeting names no repo

1. Remove `repo:` from last week's `meeting.md`, and set
   `"repos": { "default": "~/code/<a real repo>" }` in
   `~/Meetings/.app/config.jsonc`.
2. Reopen the brief. Expected: the commits come from that repo.

## 3. No repo, or not a git folder → commits left out, no error

1. Remove `repos.default` too (or point it at a plain folder).
2. Reopen the brief. Expected: the "Last time" card only, no commits section,
   no error.

## 4. A title with no earlier meeting

1. Go to `#/brief?title=Something%20New`.
2. Expected: "Nothing from last time" saying there is no earlier meeting
   called "Something New". No commits section.

## 5. Solo blocks in Today are not clickable

1. Have a calendar block with only you on it today (e.g. "Focus time").
2. Expected: it is greyed in the Today pane, clicking it does nothing, and
   Tab skips it.

## 6. Large folder stays quick

1. With a few hundred meetings in the folder, open a brief.
2. Expected: it shows in well under a second after the first search-index
   build (the lookup is one index read plus one folder read, not a scan).
