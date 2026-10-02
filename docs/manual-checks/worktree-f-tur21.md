# Manual checks: TUR-21 (Sync: remember an unsaved issue link across an app restart)

These need the running, signed app, a signed-in agent CLI and a real tracker,
so they were not run here. The headless parts are covered by tests in
`src-tauri/src/sync/tests.rs` (the `TUR-21` block at the end) and
`src-tauri/src/sync/kept.rs`.

Where it lives: `<meetings>/.app/unsaved-syncs.json`. The file exists only
while an issue is kept, and is removed once the last one is saved or
dismissed.

## 1. Failed save, quit, Retry

1. Press **Sync** on a task, and start a meetings-folder move so that it is
   still running when the agent answers (a big folder on another volume
   helps). The task shows "Created in Linear as ENG-… but could not save the
   link: …".
2. Let the move finish. Expected: `<new folder>/.app/unsaved-syncs.json`
   exists and names the task, the issue key and its address.
3. Quit the app (⌘Q) and open it again. Press **Sync** on the same task.
4. Expected: the task shows **Synced** at once, with no "Syncing…" agent run,
   and the tracker still has one issue. `unsaved-syncs.json` is gone.

## 2. The task changed while the app was closed

1. Repeat 1.1 to 1.3, but before opening the app again edit the task's file
   in a text editor (or press **Make notes now** for its meeting after
   opening, before pressing Sync).
2. Expected: Sync runs the agent as usual and makes a new issue for the task
   as it is now. `meet-ai.log` has a line "TICK-… changed since the app kept
   its issue https://…; syncing it afresh", so the first issue's address is
   not lost. This is what the ticket asks for ("stale entry → dropped, normal
   sync"). Note it differs from a change seen *without* a restart (TUR-20),
   which keeps the issue and shows **Dismiss**.

## 3. Dismiss, quit, Sync

1. Make a save fail so that the task shows **Dismiss** (TUR-20 manual check
   2.5), and press **Dismiss**.
2. Quit and reopen the app, press **Sync**. Expected: a normal agent run; the
   dismissed issue is not written to the task.

## 4. A broken file

1. Quit the app. Replace `<meetings>/.app/unsaved-syncs.json` with `{ x`.
2. Open the app and press **Sync** on any task. Expected: an error naming
   the file ("Could not read …/unsaved-syncs.json: … fix or delete it, then
   press Retry"), no agent run, and the file left as it was. After deleting
   the file, Sync works as usual.

## Known limit

If the app quits *while* the folder move that refused the save is still
running, the kept issue is lost: nothing may be written under the meetings
root during a move (the move deletes the old folder afterwards), and the app
writes nothing outside it (SPEC L10). The error text still shows the issue's
address. A move that ends normally writes the file (check 1.2).

## Decisions taken without an answer

- The file is read on the first Sync, Retry or Dismiss after the app starts,
  not in the app's setup: that is the first time anything needs it, and it
  keeps a broken file from slowing or blocking the start. Reading needs no
  folder-move gate (only writes do), so a Sync pressed during a move still
  finds a kept issue.
- A file that cannot be read (bad JSON, unknown version, read error) blocks
  Sync with an error naming it, instead of being ignored: ignoring it could
  make a second issue, and writing over it could lose a kept one. The new
  error kind is `sync-kept-unreadable`; the window shows its message like any
  other Sync error.
- The ticket fingerprint is now SHA-256 of the ticket file (it was std's
  `DefaultHasher`, which Rust does not promise to keep the same between
  releases, so a saved fingerprint could stop matching after an update).
