# Manual checks: tur173

TUR-173: the window's IPC types can no longer drift from Rust without
`pnpm typecheck` noticing.

- `narrow()` is gone. Rust sends enums where it sent strings
  (`UiError.domain`, `TicketSummary.status`, `BriefTicket.status`,
  `TranscriptLine.speaker`, `SelectionView.engine`, `TrackerSettings.tracker`
  and `.harness`, `TicketSyncOverview.tracker`), so every command goes
  through `call()` with its generated type.
- `src/ipc/types.ts` only renames generated types; no hand-written copies.
- Every emitted event payload is generated (`ProgressEvent`, `watch::Changed`
  and `stt::session::LiveUpdate` were not), and `subscribe` takes a typed
  `EventMap` keyed by the generated event constants (`src/ipc/events.ts`). A
  new event in `events.rs` with no entry there fails typecheck.
- The no-backend defaults come from Rust (`DEFAULT_*` constants in
  `bindings.ts`, built in `src-tauri/src/ipc_defaults.rs` from the config
  `Default`s).
- A failed `listen()` is logged through `@tauri-apps/plugin-log`, or the
  console when the log cannot be written.
- `headphone_warning` and `hooks::app` read `Recorder::status()` instead of
  re-parsing the recorder's event JSON; the notes-done hook reads the event
  back into `agent_run::Status`.

Ran headless here: `cargo test -p meet-ai -p store -p stt`, clippy and fmt,
`just bindings`, `pnpm typecheck`, the whole vitest suite, biome, and the
quality gate. A rename check: renaming `ProgressEvent::downloaded_bytes` in
Rust and regenerating made `pnpm typecheck` fail in
`src/ui/engine/DownloadProgress.tsx` (reverted). No app was launched.

## Run by hand

Use a signed build of this branch.

1. **Recording still works end to end.** Press ⌘⇧R, speak, press ⌘⇧R again.
   Expected: the live pane fills, the meeting opens with `You` lines, the
   timer and the menu-bar item follow the phase, as before.
2. **Headphone warning.** Record through the built-in speakers with
   `audio.warn_no_headphones` on. Expected: the "No headphones" banner shows
   while recording and clears when it stops (it now reads the recorder's own
   status).
3. **Hooks.** Set `hooks.on_meeting_end` and `hooks.on_analysis_complete` to a
   command that fails (`false`). Record a short meeting and let the notes run.
   Expected: two "hook failed" notes, one per hook, as before.
4. **Tracker settings.** Settings, Tracker: pick Jira, save, reopen.
   Expected: Jira is shown. Hand-edit `config.jsonc` to
   `"tickets": { "tracker": "asana", "tracker_mcp": "x" }` and reopen.
   Expected: Linear is shown (an unknown tracker now reads as the default; it
   used to reach the window as `asana`), and the log has
   `tickets.tracker is not one meet-ai knows`.
5. **Model download progress.** Settings, Speech: download a model.
   Expected: the bar moves and goes indeterminate while verifying.
6. **A failed listen is logged.** Not reproducible on a normal build (every
   window has `core:event:allow-listen`). To see it, remove
   `core:event:allow-listen` from `capabilities/default.json` in a local
   build and open the app. Expected: `could not listen for recording://state`
   lines in the log (Settings, Open logs folder).

Not changed, follow-up: the prompt and overlay windows have no
`log:allow-log` (TUR-158 keeps their capabilities minimal), so a failed
listen there reaches only the webview console, not the log file. Granting it
is a capability change left to the owner.
