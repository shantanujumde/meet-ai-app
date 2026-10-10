# Manual checks: TUR-176

Pure refactor of duplicated helpers in `src-tauri` (and `store`, `agent`):
one `worker::panic_message`, one stoppable `worker::spawn`, one
`notify::post`, one `lock::lock_or_recover`, one `tickets::find` /
`tickets::files` / `tickets::summary_at`, one `store::meeting::display_title`
/ `display_date` / `folder_title`, one `agent::detect::expand_home`, and a
config `Section` trait, `section::Flag` and `ConfigError` in
`config/error.rs`. Everything is covered by the existing unit tests, which
run headless (`cargo test -p meet-ai -p store -p agent`). Nothing below was
run here: each needs the running, signed app.

## Run by hand

1. Close the main window for the first time after a fresh install.
   Expect: the one-off "meet-ai is still running" notification, as before.
   Why skipped: needs the running app and notification permission.
2. Press ⌘⇧R with no microphone permission (or another refused start).
   Expect: the "meet-ai did not start recording" notification with the reason.
   Why skipped: needs the running app and a refused start.
3. Open a meeting app (or wait for a calendar reminder) with the popup unable
   to show. Expect: the "Record this meeting?" notification as the fallback.
   Why skipped: needs the running app, a meeting app or a calendar event.
4. Let a notes run finish with the window in the background.
   Expect: "Notes are ready" with "<title>: the notes and N tasks are written."
   Why skipped: needs a signed-in agent CLI and a recorded meeting.
5. Record for a minute, stop with ⌘⇧R. Expect: the recording stops at once
   (the ticker wakes immediately) and the WAVs are complete.
   Why skipped: needs a signed build with microphone and system-audio grants.
6. Leave the app running across a calendar event with a reminder lead time.
   Expect: the reminder fires once, and quitting the app does not hang.
   Why skipped: needs the running app and a calendar.
7. Copy prompt → Start work on a ticket, and Sync a ticket, both for a meeting
   with no `meeting.md` title. Expect: the meeting is named from its folder
   ("Platform standup"), with the folder's date and time, as before.
   Why skipped: needs the running app (and a signed-in agent CLI for Sync).
8. Windows only: a meeting `repo` written as `~\apps\api` in `meeting.md`.
   Expect: the brief reads that repo's commits (it now goes through the same
   `expand_home` as `agent.binary_path`, which splits on either slash there;
   before, only `~/` was expanded). Why skipped: needs a Windows machine.

## Small differences, on purpose

- A notification that cannot be shown now logs one line, "could not show a
  notification", with its title as a field, instead of a different sentence
  per notification.
- A speech engine that panics with a payload that is not text now reads
  "the speech engine crashed (no message)" instead of "(unknown error)", the
  same fallback the recorder's ticker uses.
