# Manual checks: TUR-20 (Sync: no duplicate issue after a folder move, no local path in the prompt)

These need the running, signed app, a signed-in agent CLI and a real tracker,
so they were not run here. The headless parts are covered by tests in
`src-tauri/src/sync/tests.rs` and `crates/prompts/src/push_ticket.rs`.

## 1. Folder moved during a Sync

1. Open a meeting with a task. Press **Sync**.
2. While it says "Syncing…", go to Settings and change the meetings folder.
3. Expected: once the move is done and the agent answers, the task shows
   **Synced** with its issue key, and the ticket file in the *new* folder has
   `synced_to`, `external_id` and `external_url`. Only one issue exists in the
   tracker.

## 2. Save refused, then Retry

1. Press **Sync**, and start a meetings-folder move so that it is still
   running when the agent answers (a big folder on another volume helps).
2. Expected: the task shows "Created in Linear as ENG-… but could not save the
   link: … The issue is at https://…. Press Retry to save the link; it will
   not create another issue."
3. After the move ends, press **Retry**. Expected: the task shows **Synced**
   at once (no agent run), and the tracker still has one issue.

Known limit: the not-yet-saved issue is kept in memory only. If the app quits
between the failed save and Retry, Retry runs the agent again and can make a
second issue. The error text holds the issue's address, so the user can still
find it.

## 3. No local path in the issue

1. Sync a task to Linear for real.
2. Expected: the issue's description names the meeting by its title and date
   (and folder name), and has no `/Users/<you>/…` path and no `meeting.md`.

## 4. A saved `push-ticket.md` that uses `{{ meeting_file }}`

1. Put a custom `<meetings>/.app/prompts/push-ticket.md` that still contains
   `{{ meeting_file }}`, then Sync.
2. Expected: Sync runs as usual; that variable is now always empty.
