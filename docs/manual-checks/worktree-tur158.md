# Manual checks: TUR-158

Each window may call only what its own page uses (SPEC A36): `build.rs`
declares the app commands (`src-tauri/src/app_commands.rs`), and the capability
files grant each window its own list. Unit tests check the lists against
`bindings.ts` and the capability files; the build itself fails on a capability
that names an unknown command (tried here by granting the removed
`allow-calendar-sign-out`: the build script failed). Nothing below was run here:
each needs the signed app.

## Run by hand

1. `just bundle-signed`, launch, and use the main window: list, open, rename
   and delete a meeting, edit notes, change the meetings folder (folder picker
   and the confirm dialog), copy a prompt, Settings (every section), Tickets,
   calendar connect and disconnect, drag the window by its title bar and
   double-click the title bar.
   Expect: all work as before; no "not allowed by ACL" line in meet-ai.log.
   Why skipped: needs the signed app.
2. Settings → About: click the Parakeet credit link; Settings → Speech with a
   whisper model: click the languages link.
   Expect: each opens in the browser. Any other https link the page might try
   would be refused.
   Why skipped: needs the running app and a browser.
3. Onboarding / Settings: "Open System Settings" (and on Windows, the
   `ms-settings:` page).
   Expect: the settings pane opens. It goes through Rust
   (`open_privacy_settings`), which the webview's opener scope no longer
   covers and does not need.
   Why skipped: needs the running app.
4. A meeting reminder or a detected call: the "Record this meeting?" card.
   Expect: it shows its card in the saved Light / Dark look, Record and the
   chevron menu work. From its web inspector (debug build),
   `__TAURI_INTERNALS__.invoke("list_meetings")` is refused.
   Why skipped: needs a calendar event or a meeting app, and the running app.
5. Start a recording with the overlay on.
   Expect: the overlay shows the timer and live lines, Pause / Resume / Stop
   and "show main window" work, and it drags by its card. Calling
   `list_meetings` from its inspector is refused.
   Why skipped: needs a mic and the signed app.
6. With meet-ai running behind the overlay or the card, launch it again.
   Expect: the main window comes forward (by its label), never the card or
   the hidden overlay.
   Why skipped: needs the running app.
7. Notifications, ⌘⇧R and the tray still work.
   Expect: unchanged: they are driven from Rust, which capabilities do not
   restrict.
   Why skipped: needs the signed app.

## Known

- `tauri-plugin-fs` stays in `Cargo.lock`: `tauri-plugin-dialog` depends on
  it. Only meet-ai's own dependency and its `.plugin(...)` line are gone.
