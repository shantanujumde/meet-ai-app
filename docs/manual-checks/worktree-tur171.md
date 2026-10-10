# Manual checks: tur171

TUR-171: the window made slow or repeated backend calls. What changed:

- **One probe for the engine card.** `EngineChoices` carries `selectionError`
  (why the saved engine cannot run), from the same `meet-stt --probe` as the
  picker (`stt::registry::options_and_resolve`). The card no longer calls
  `engine_selection`, before or after a save. The command itself is kept.
- **One config read for Settings.** `settings_snapshot` returns every saved
  value the Settings cards show (app, audio, agent, notes, notifications,
  calendar sources, tracker, the appearance and detection config problems),
  read inside `config::read_once`: the root pointer and `config.jsonc` are
  read once and every section reader uses that text. The cards take their
  first value from it (`src/ui/settings/snapshot.tsx`) and still save through
  their own commands. Outside Settings (onboarding, a card's own test) a card
  asks its own command as before, and so does a card whose snapshot failed.
- **CLI checks and the meeting view's config answers kept per session**
  (`src/state/session.ts`): agent detection and the tracker servers run once,
  again on "Check again", and after a save that changes them. The meeting view
  asks `copyPromptFallback`, `agentChoice`, `detectAgents` and `notesAutoRun`
  once per session; Settings' saves and each Settings snapshot update them.
- **Live meeting view**: only the live pane subscribes to the live transcript.
- **Meetings page**: one `TodayPane` in one place for every state.
- **Menu-bar navigation**: subscribed once; `navigate` is read from a ref.

Not done (the ticket marks it optional): caching config reads on file mtime
for the hot paths (window close, output change, permission check). Those
still read the file each time.

Ran headless here: `cargo test -p meet-ai` and `-p stt`, clippy, fmt, `just
bindings`, `pnpm typecheck`, biome, the whole vitest suite (render-count tests
for the meeting view, call-count tests for Settings, the Meetings page and the
menu-bar listener), and the quality gate. No app was launched.

## Run by hand

Use a build of this branch on macOS.

1. **Settings reads once.** Set `RUST_LOG=meet_ai=debug` if you want the IPC
   trace, open Settings, and check in the webview devtools (Network/console or
   a breakpoint on `__TAURI_INVOKE`) that `settings_snapshot` is called once
   and `app_settings`, `menu_bar_countdown`, `notification_settings`,
   `tracker_settings`, `calendar_sources`, `config_problem`, `agent_choice`,
   `notes_auto_run` are not. Every card shows the same values as on `main`.
2. **One probe.** On the same visit, `engine_choices` is called once and
   `engine_selection` not at all. With `transcription.engine` set to
   `"whisper"` and no model downloaded, the "This Mac will use the
   downloadable speech model" row still shows.
3. **CLI checks are kept.** Leave Settings and open it again: the agent rows
   and the tracker server list show at once, with no "Checking…" and no
   `detect_agents` / `tracker_servers` call. "Check again" runs each again.
   Switch the agent from Claude Code to Codex: the next Tracker visit asks for
   Codex's servers.
4. **A hand edit shows on the next visit.** With Settings closed, change
   `app.show_in_dock_when_closed` in `config.jsonc` by hand, then open
   Settings: the switch shows the new value.
5. **Live meeting.** Record a meeting with its page open and use the React
   profiler (or React DevTools "Highlight updates"): per transcript event only
   the live transcript pane renders, not the header, the notes switch or the
   notes pane. Opening another, finished meeting while recording: nothing on
   its page renders per event.
6. **Today pane.** Launch the app with meetings in the folder: "Reading your
   calendar…" shows once, and `todays_meetings` is called once at launch.
7. **Open brief from the menu bar.** Click around between meetings and
   Settings, then pick "Open brief" in the menu bar: the brief opens every
   time.
