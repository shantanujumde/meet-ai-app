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

4. Variant: after step 2, move the task's file out of its folder, press
   Retry, then put it back and press Retry again. Expected: the first Retry
   says the link could not be saved and shows the issue's address; the second
   saves it, with no agent run either time.
5. Variant: after step 2, sync the task by hand (add `external_url` to its
   file), or press **Make notes now** so its number goes to a different task,
   then press Retry. Expected: "Created in Linear but couldn't attach it to
   this task: https://… …", a **Dismiss** button, and no agent run, however
   often Retry is pressed. After **Dismiss**, Sync runs afresh.
6. Variant: with two meetings that both have a `TICK-0001`, make the first
   one's save fail, then Sync the second one. Expected: the second gets its
   own new issue; the first's Retry still saves the first issue.

## 2b. The task changes while it syncs

The app has no way to edit an existing task, so nothing in the app is locked
during a sync. A change from outside the app still cannot cost an issue:

1. Press **Sync**, and while it says "Syncing…" edit the task's file in a
   text editor (or press **Make notes now** for its meeting).
2. Expected: when the agent answers, the task shows "Created in Linear but
   couldn't attach it to this task: https://… TICK-… changed after the sync
   started." with Retry and **Dismiss**. The edit is kept and the file gets no
   link. Retry does not run the agent; the tracker has one issue.

Known limit (follow-up): the not-yet-saved issue is kept in memory only. If
the app quits before it is saved or dismissed, the next Sync runs the agent
again and can make a second issue. The error text holds the issue's address,
so the user can still find it. Also a follow-up: the address in these errors
is plain text, not a clickable Open button.

## 3. No local path in the issue

1. Sync a task to Linear for real.
2. Expected: the issue's description names the meeting by its title and date
   (and folder name), and has no `/Users/<you>/…` path and no `meeting.md`.

## 4. A saved `push-ticket.md` that uses `{{ meeting_file }}`

1. Put a custom `<meetings>/.app/prompts/push-ticket.md` that still contains
   `{{ meeting_file }}`, then Sync.
2. Expected: Sync runs as usual; that variable is now always empty.
