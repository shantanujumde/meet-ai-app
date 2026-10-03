# Changelog

All notable changes to meet-ai are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Distribution is **personal only** at this stage — SPEC.md §8.1 puts public
release in v2. Builds are signed with a self-signed local identity, not a
Developer ID, and are not notarized.

## [0.4.0](https://github.com/shantanujumde/meet-ai-app/compare/v0.3.0...v0.4.0) (2026-10-03)


### Features

* agent setup step and Settings card, with Test (TUR-9) ([#48](https://github.com/shantanujumde/meet-ai-app/issues/48)) ([41954d2](https://github.com/shantanujumde/meet-ai-app/commit/41954d2c85c6da456b860eca90c7795d35c07c7a))
* **agent:** Claude Code runner, notes run and sync run (TUR-4) ([#44](https://github.com/shantanujumde/meet-ai-app/issues/44)) ([465e0b1](https://github.com/shantanujumde/meet-ai-app/commit/465e0b17c8cdb2c4f1f6a1588638133fc66a7155))
* **agent:** Codex harness for the notes and sync runs (TUR-5) ([#46](https://github.com/shantanujumde/meet-ai-app/issues/46)) ([4aa4918](https://github.com/shantanujumde/meet-ai-app/commit/4aa49188eb31525ca9c55df7b673250fbd472998))
* **agent:** Codex sync pre-approves only the tracker's tools (TUR-16) ([#53](https://github.com/shantanujumde/meet-ai-app/issues/53)) ([c3cfdf7](https://github.com/shantanujumde/meet-ai-app/commit/c3cfdf7807d7ff4a3fdad0c466f13714418d040d))
* **agent:** default to the CLI's own model and list every model it offers (TUR-74) ([#84](https://github.com/shantanujumde/meet-ai-app/issues/84)) ([6873b3d](https://github.com/shantanujumde/meet-ai-app/commit/6873b3d9f9ff7678631053ae976449a06bd648d0))
* **agent:** find installed agent CLIs and their sign-in state (TUR-6) ([#45](https://github.com/shantanujumde/meet-ai-app/issues/45)) ([259a6f5](https://github.com/shantanujumde/meet-ai-app/commit/259a6f55567e236dbec9821532717a21d82ba57b))
* **agent:** Harness trait, job types, time limit and cancel (TUR-1) ([#41](https://github.com/shantanujumde/meet-ai-app/issues/41)) ([c702bc2](https://github.com/shantanujumde/meet-ai-app/commit/c702bc2b53819cb2fb8467e4a7b48c7ccf327028))
* **app:** keep running in the menu bar when the main window is closed (TUR-76) ([#82](https://github.com/shantanujumde/meet-ai-app/issues/82)) ([e56167c](https://github.com/shantanujumde/meet-ai-app/commit/e56167c480dad33c80062062f0d9915c24dae61d))
* **brief:** pre-meeting brief with last time's notes and recent commits (TUR-32) ([#66](https://github.com/shantanujumde/meet-ai-app/issues/66)) ([74e25ba](https://github.com/shantanujumde/meet-ai-app/commit/74e25ba8e7daf1cdb55fa610db3c224e410180c6))
* **calendar:** calendar settings, Calendar app + Google/Microsoft sign-in (TUR-49) ([#89](https://github.com/shantanujumde/meet-ai-app/issues/89)) ([74c8c9f](https://github.com/shantanujumde/meet-ai-app/commit/74c8c9f61abc5339b00bd620921f80ac82a8051e))
* **calendar:** read a Microsoft calendar through Graph calendarView (TUR-47) ([#86](https://github.com/shantanujumde/meet-ai-app/issues/86)) ([d16fada](https://github.com/shantanujumde/meet-ai-app/commit/d16fada62f02bd9836758d77c38b916b770574f4))
* **calendar:** read events from EventKit (TUR-26) ([#58](https://github.com/shantanujumde/meet-ai-app/issues/58)) ([b3f5ade](https://github.com/shantanujumde/meet-ai-app/commit/b3f5ade2915b12acec54e23ce0155d093a7bea87))
* **calendar:** read Google Calendar events with singleEvents=true (TUR-48) ([#87](https://github.com/shantanujumde/meet-ai-app/issues/87)) ([653797e](https://github.com/shantanujumde/meet-ai-app/commit/653797e8c2606bebf49ba78e42716dec26ce9c09))
* **calendar:** sign in with Google and Microsoft, PKCE loopback, token in the OS keystore (TUR-44) ([#75](https://github.com/shantanujumde/meet-ai-app/issues/75)) ([18be009](https://github.com/shantanujumde/meet-ai-app/commit/18be009018616f57ece35617775193eebdc824b8))
* **calendar:** today's meetings pane, straight from the calendar (TUR-28) ([#64](https://github.com/shantanujumde/meet-ai-app/issues/64)) ([747bc3b](https://github.com/shantanujumde/meet-ai-app/commit/747bc3bd955ac0a7e8e29c576dd097b96f6f5ad3))
* **config:** agent and tickets sections in config.jsonc, with JSON schema (TUR-3) ([#38](https://github.com/shantanujumde/meet-ai-app/issues/38)) ([36e78b8](https://github.com/shantanujumde/meet-ai-app/commit/36e78b88c7db4d725a92507d3cdbc539dddb8660))
* **config:** read the calendar and detection sections (TUR-25) ([#57](https://github.com/shantanujumde/meet-ai-app/issues/57)) ([6167607](https://github.com/shantanujumde/meet-ai-app/commit/61676076101eb6f54cd557a9b1fe382eeeadb6d9))
* copy-prompt paths, Start Work and the no-agent fallback (TUR-8) ([#42](https://github.com/shantanujumde/meet-ai-app/issues/42)) ([fcfc093](https://github.com/shantanujumde/meet-ai-app/commit/fcfc09369e51064e2f593e8e52cd5dcd9ed17290))
* **detection:** ask when the mic and speakers are both in use, like a call (TUR-31) ([#65](https://github.com/shantanujumde/meet-ai-app/issues/65)) ([8dc5ddf](https://github.com/shantanujumde/meet-ai-app/commit/8dc5ddff972ebf8c6ead9e8a792041c33a06ae83))
* **detection:** remind a minute before each meeting, with Record and the brief (TUR-30) ([#69](https://github.com/shantanujumde/meet-ai-app/issues/69)) ([1bd5228](https://github.com/shantanujumde/meet-ai-app/commit/1bd52280a2c3f992f2a7f94555adc6ebdedd6d3e))
* **detection:** spot a running meeting app and ask before recording (TUR-27) ([#60](https://github.com/shantanujumde/meet-ai-app/issues/60)) ([b6dd72f](https://github.com/shantanujumde/meet-ai-app/commit/b6dd72fe0e48fea11e494ae4f11577f64c01a57f))
* folder watcher (TUR-100) ([#24](https://github.com/shantanujumde/meet-ai-app/issues/24)) ([3125efa](https://github.com/shantanujumde/meet-ai-app/commit/3125efa44044826992bc611e7ab70226cbc528a6))
* **ipc:** generate TypeScript bindings with tauri-specta (Phase 2) ([#29](https://github.com/shantanujumde/meet-ai-app/issues/29)) ([af46fb9](https://github.com/shantanujumde/meet-ai-app/commit/af46fb906a1bc9f17fb62965fe867f82bad7f5c8))
* **logs:** logs in .app/logs with rotation, plus local crash files (TUR-46) ([#72](https://github.com/shantanujumde/meet-ai-app/issues/72)) ([a24281a](https://github.com/shantanujumde/meet-ai-app/commit/a24281ace0122b546b84bf1e40b6e558f483a2cf))
* meeting search, derived FTS5 index (TUR-101) ([#27](https://github.com/shantanujumde/meet-ai-app/issues/27)) ([4d213d9](https://github.com/shantanujumde/meet-ai-app/commit/4d213d926d28232aedd586da37980a3363008d22))
* notes start on their own when a call ends, with status, Retry and Cancel (TUR-10) ([#47](https://github.com/shantanujumde/meet-ai-app/issues/47)) ([62d5f8a](https://github.com/shantanujumde/meet-ai-app/commit/62d5f8a6a594b6cd75825805e9e400cd2a5bba24))
* per-meeting Make notes switch and Make notes now (TUR-12) ([#50](https://github.com/shantanujumde/meet-ai-app/issues/50)) ([1816ee7](https://github.com/shantanujumde/meet-ai-app/commit/1816ee7cf0e2676c67fdb6e65fe56ed347485d91))
* **prompts:** notes schema and wrap-up.md prompt template (TUR-2) ([#40](https://github.com/shantanujumde/meet-ai-app/issues/40)) ([da60bbc](https://github.com/shantanujumde/meet-ai-app/commit/da60bbcb36c84812d39289842eeb807279e90510))
* **recording:** name the recording from its calendar event (TUR-29) ([#67](https://github.com/shantanujumde/meet-ai-app/issues/67)) ([30d2bae](https://github.com/shantanujumde/meet-ai-app/commit/30d2bae7925407875fce3aa7c6abb8ad28c9dcab))
* **retention:** delete meeting audio after audio.retention_days (TUR-45) ([#77](https://github.com/shantanujumde/meet-ai-app/issues/77)) ([8c74207](https://github.com/shantanujumde/meet-ai-app/commit/8c74207fd84c45dbe24b212412041ec7fd296e63))
* **review:** calm meeting header, notes switch moves to Meeting notes (TUR-81) ([#80](https://github.com/shantanujumde/meet-ai-app/issues/80)) ([712d1e5](https://github.com/shantanujumde/meet-ai-app/commit/712d1e5b22f784dc397db82e39b36147c60f39c9))
* **settings:** Notifications card, reminder lead time and a Join button (TUR-78) ([#92](https://github.com/shantanujumde/meet-ai-app/issues/92)) ([e58c95e](https://github.com/shantanujumde/meet-ai-app/commit/e58c95ef5cbd9237248f1d084d51b96f9ba01bb9))
* **settings:** pick the speech engine and model, and say what each is good for (TUR-75, TUR-79) ([#85](https://github.com/shantanujumde/meet-ai-app/issues/85)) ([e956f38](https://github.com/shantanujumde/meet-ai-app/commit/e956f3841e395ffab852b6560c09c18a7a68460b))
* **store:** write notes and tickets from the agent's JSON (TUR-7) ([#43](https://github.com/shantanujumde/meet-ai-app/issues/43)) ([dd0f335](https://github.com/shantanujumde/meet-ai-app/commit/dd0f335996ab0d061b1c4ae4264db18b7dc2ffc5))
* Sync tasks to Linear / Jira / GitHub, per task and Sync all (TUR-11) ([#49](https://github.com/shantanujumde/meet-ai-app/issues/49)) ([a627aa6](https://github.com/shantanujumde/meet-ai-app/commit/a627aa65806bcd4c3e54367ab3cacff52bc5984d))
* **sync:** remember an unsaved issue link across an app restart (TUR-21) ([#56](https://github.com/shantanujumde/meet-ai-app/issues/56)) ([98d974f](https://github.com/shantanujumde/meet-ai-app/commit/98d974f94ca934a569b7a0ec3aaf6f42ede57541))
* tickets screen and new-ticket form (TUR-102) ([#23](https://github.com/shantanujumde/meet-ai-app/issues/23)) ([253273a](https://github.com/shantanujumde/meet-ai-app/commit/253273a6d3323db6f9e2bcbb5bf8cea4f31b61b2))
* **tray:** today's meetings in the menu bar, with Join and Record (TUR-77) ([#88](https://github.com/shantanujumde/meet-ai-app/issues/88)) ([7c46d18](https://github.com/shantanujumde/meet-ai-app/commit/7c46d18989eabbc2a1eb23184a32338ba7b5d20d))


### Bug Fixes

* address review of [#15](https://github.com/shantanujumde/meet-ai-app/issues/15) (folder-move gate, download claim, root rules) ([efd373c](https://github.com/shantanujumde/meet-ai-app/commit/efd373c4509e6ca6ae8b607c437277e2fc9c3bf9))
* **agent:** make the agent picker radios 16 px and the whole row clickable (TUR-73) ([#76](https://github.com/shantanujumde/meet-ai-app/issues/76)) ([fce668e](https://github.com/shantanujumde/meet-ai-app/commit/fce668e2b027c7295fd9aaee48485e1296c38edd))
* **audio:** count the permission check's deadline from the first tap frame (TUR-72) ([#73](https://github.com/shantanujumde/meet-ai-app/issues/73)) ([9124266](https://github.com/shantanujumde/meet-ai-app/commit/9124266898bf7ad850ae159c345357d6836fc68f))
* **audio:** keep recording the mic through headset tap faults (TUR-87) ([#95](https://github.com/shantanujumde/meet-ai-app/issues/95)) ([6671da2](https://github.com/shantanujumde/meet-ai-app/commit/6671da22d48e81305b3706a944ccb73bebf341c5))
* **audio:** play the recording-start chime once (TUR-14) ([#39](https://github.com/shantanujumde/meet-ai-app/issues/39)) ([599992b](https://github.com/shantanujumde/meet-ai-app/commit/599992b6170dbc5f8ac0f137cea681744bf35a97))
* **audio:** real system audio counts as granted, and the tap uses the aggregate's measured rate (TUR-84) ([#91](https://github.com/shantanujumde/meet-ai-app/issues/91)) ([e33add1](https://github.com/shantanujumde/meet-ai-app/commit/e33add11e84ebefba78f8acc6fad790890809823))
* **calendar:** cloud-only reminders, blip-proof menu bar, Microsoft 1:1s and account names (TUR-88) ([#97](https://github.com/shantanujumde/meet-ai-app/issues/97)) ([d06aa41](https://github.com/shantanujumde/meet-ai-app/commit/d06aa4129cd4ce7b42768a95bc8a3646f4bb47f3))
* **calendar:** Join links are checked the way a browser reads them, and cloud events find pasted links (TUR-86) ([#93](https://github.com/shantanujumde/meet-ai-app/issues/93)) ([5d18339](https://github.com/shantanujumde/meet-ai-app/commit/5d183395aa91bd4bf3aca5c725aac2bdd513c604))
* gate every write under the meetings root during a folder move ([a8b1bbb](https://github.com/shantanujumde/meet-ai-app/commit/a8b1bbba3bb028c698b350c098c837b943cd64bf))
* keep blocking work off the main thread; models follow the meetings root ([9998d38](https://github.com/shantanujumde/meet-ai-app/commit/9998d381fb449dd3272229b3ac35a6fae2caab94))
* main-thread blocking + model dir ([0e97be4](https://github.com/shantanujumde/meet-ai-app/commit/0e97be495c4fb46bcfd51f213b9a062578af805e))
* **modelfetch:** fail and retry a download that stalls mid-body (TUR-22) ([#59](https://github.com/shantanujumde/meet-ai-app/issues/59)) ([cd14176](https://github.com/shantanujumde/meet-ai-app/commit/cd14176d94037d99582a79a3f652fcfd7559e18e))
* **notes:** wait for the final transcript, no run when notes are off, survive a folder move (TUR-17) ([#54](https://github.com/shantanujumde/meet-ai-app/issues/54)) ([994a30f](https://github.com/shantanujumde/meet-ai-app/commit/994a30f653f1e99bfc1e0a85262c04d6d84ec3a7))
* **quality:** R10 over the whole tree, R9 notices recheck and allow-list, honest docs (TUR-89) ([#96](https://github.com/shantanujumde/meet-ai-app/issues/96)) ([d048202](https://github.com/shantanujumde/meet-ai-app/commit/d04820232795354d4cab5cfeded6c5a50c7195cc))
* **recording:** one error path — the reason a recording ended rides on its status ([b350977](https://github.com/shantanujumde/meet-ai-app/commit/b350977ed45fcbb5c322c67a757517110525d6a2))
* **recording:** say why a recording ended on its status, not a second event ([406abfb](https://github.com/shantanujumde/meet-ai-app/commit/406abfb18339c740d1bc1b3008bbb92460a5bc13))
* **retention:** never delete audio it can't prove is safe (TUR-85) ([#94](https://github.com/shantanujumde/meet-ai-app/issues/94)) ([846c779](https://github.com/shantanujumde/meet-ai-app/commit/846c77942b78a9a3424ffebe9dbb1f3495b8e6f0))
* **store:** judge self-write suppression when the event was seen (TUR-83) ([#90](https://github.com/shantanujumde/meet-ai-app/issues/90)) ([b05e8b4](https://github.com/shantanujumde/meet-ai-app/commit/b05e8b4a590feec7f4fd1d4f304119dfe23d167e))
* **stt:** name the missing whisper model and the installed ones (TUR-23) ([#62](https://github.com/shantanujumde/meet-ai-app/issues/62)) ([c77b8b8](https://github.com/shantanujumde/meet-ai-app/commit/c77b8b892f13ea2234f218c195f07e95eb602514))
* **sync:** no duplicate issue after a folder move, no local path in the prompt (TUR-20) ([#55](https://github.com/shantanujumde/meet-ai-app/issues/55)) ([647b2ab](https://github.com/shantanujumde/meet-ai-app/commit/647b2ab4f50b3f82623a2435127f38db8f4faeff))
* **tickets:** number hand-made tickets under the store lock (TUR-18) ([#51](https://github.com/shantanujumde/meet-ai-app/issues/51)) ([ab2127d](https://github.com/shantanujumde/meet-ai-app/commit/ab2127d01a99484ac3e7b517c738ecac11ceed13))
* **ui:** address PR [#19](https://github.com/shantanujumde/meet-ai-app/issues/19) review ([4781a22](https://github.com/shantanujumde/meet-ai-app/commit/4781a22df698faaa35e0614f28ca8fbeb9341738))
* **ui:** lock the document scroll so only panes scroll (TUR-15) ([#37](https://github.com/shantanujumde/meet-ai-app/issues/37)) ([1e5ba18](https://github.com/shantanujumde/meet-ai-app/commit/1e5ba18349fee479dfe20a922c02079518646479))
* **ui:** reset the content pane's scroll on every route change (TUR-82) ([#79](https://github.com/shantanujumde/meet-ai-app/issues/79)) ([ebff1a6](https://github.com/shantanujumde/meet-ai-app/commit/ebff1a6bda43f5e39ca595e759bcb2ffcfc47aba))


### Performance Improvements

* audio path speed, modelfetch retries (Wave 2 Phase 5) ([#28](https://github.com/shantanujumde/meet-ai-app/issues/28)) ([239bf3e](https://github.com/shantanujumde/meet-ai-app/commit/239bf3e3f0aa81aab11b40a3d8d8c7a17eb2b61d))

## [Unreleased]

### Added

- `crates/store` reads and writes the meetings folder (Phase 3a, TUR-99):
  `meeting.md` with its frontmatter and four sections, `tickets/TICK-NNNN.md`,
  `notes.md`, and read-only `transcript.md`. Frontmatter keys the app does not
  know survive a rewrite. A malformed file loads with a "needs attention"
  problem attached instead of failing, and never stops the files or meetings
  around it from loading. A file that loaded broken or was not UTF-8 text is
  refused on write rather than overwritten.
  - Frontmatter that nests more than 64 levels deep or uses YAML aliases
    (`*name`) is flagged as broken. Both shapes could crash the app or fill
    memory from a file of a few kilobytes.
  - YAML comments inside frontmatter are dropped when a file is rewritten,
    and list and quoting style may change; values are kept.

### Changed

- The meeting list and review view read through `crates/store` instead of the
  app shell's own parser. What the screens show is unchanged, with one
  exception: a transcript line with no space after the speaker (`You:text`)
  now counts as unparsed, matching the SPEC §3.4 pattern exactly.

### Fixed

- A silent or quiet stretch of a meeting could get a made-up line in its
  transcript on the Apple speech engine (`You: I` over room noise). Apple's
  results now pass the same speech detector the whisper engine already uses:
  a line is kept only if its time range overlaps audio the detector heard as
  speech. The same check stops the live pane flashing that word.
- The room-noise test fixture is now generated from a fixed seed. Before, it
  drew new noise on every regeneration, and the silence tests passed or failed
  depending on the draw.

## [0.3.0] — 2026-09-30

The Record button now records for real, and the permission check in
onboarding measures real access instead of always saying "not checked".

### Added

- Recording from the app. The Record button (and ⌘⇧R) opens the microphone
  and the system-audio tap and writes `mic.wav`, `system.wav` and
  `segments.json` into the meeting folder; stop closes both cleanly. The
  capture loop moved out of the `meet-rec` binary into
  `audio::session::RecordingSession`, which `meet-rec` now also uses.
  Starting and stopping run off the UI thread, and a double-tapped shortcut
  is ignored rather than opening a second tap.
- Start refuses with a clear reason when permission is denied, and removes
  the empty meeting folder it would otherwise leave. A tap that fails to
  open is reported as an error instead of sitting in "Recording" with
  nothing written.
- The onboarding permission check is real. System audio is checked by
  playing the permission chime and hearing it back through the tap; the mic
  is checked with `AVCaptureDevice.authorizationStatus` first, so a stored
  "Denied" is caught even though the mic still opens and returns audio.
- A "Change…" button next to the meetings folder, in Settings and in the
  onboarding folder step. Picking a new folder moves every existing meeting
  into it (merging rather than overwriting if the folder already has
  something in it); nothing is left behind at the old location. Refused
  while a recording is in progress.
- `RELEASING.md`: the release steps, signing-keychain unlock, and recovery
  for the "Keychain Not Found" dialogs.

### Fixed

- The window can be dragged by its titlebar again. CSS
  `-webkit-app-region: drag` does nothing in Tauri's macOS webview, so the
  titlebar now uses `data-tauri-drag-region`.
- A user who had finished onboarding could be left on a stale setup page
  when the window was refocused. They are now sent back to the app.
- With the transcription engine set to Auto, the message now says why
  Apple's engine could not be used (sidecar missing, OS too old, on-device
  model not installed) instead of only "unavailable".

## [0.2.0] — 2026-09-28

Recording now survives a device switch mid-call, and the transcription
engine is chosen from `config.jsonc` instead of being hardcoded.

### Added

- Mid-recording device changes (for example, swapping to AirPods) close the
  current segment and open a new one, instead of writing one segment for the
  whole call. `meet-rec` polls the default input and output devices on its
  existing 200 ms loop and rebuilds both audio sources on a change. Verified
  on real hardware.
- `WavWriter::open_append`, so a new segment continues the same per-channel
  WAV file rather than truncating it.
- `drift-check` binary: reads `segments.json` and the WAV headers, reports
  per-channel drift, skew and boundary gaps, and exits 0/1/2 for
  pass/fail/not-measurable (SPEC §6).
- `transcription.engine` and `transcription.model` are read from
  `config.jsonc` (SPEC §3.5). A missing or broken file falls back to the
  defaults; an unknown engine name is logged and rejected rather than
  silently treated as `auto`.
- `offline_meeting` example, which runs the full transcription path over a
  copied meeting folder with one engine forced, to prove it works with the
  network off.
- `just check-whisper` documented in CONTRIBUTING.md as the manual gate for
  the real-model whisper tests, with when to run it.

### Fixed

- A force-quit could leave the WAV headers declaring more frames than
  `segments.json` accounted for. The writer now freezes the synced frame
  count at fsync time and the header only ever declares that count.
- Segment reopening read the mic position before stopping it, which could
  reproduce the same header-ahead race one layer up.

### Changed

- Whisper word-error-rate threshold tightened from 30% to 15% now that it has
  been measured (3.2% on `mic.wav`, 0.0% on `system.wav` with
  `small.en-q5_1`).
- The whisper silence-hallucination guard has now been run against the real
  model and holds without changes (SPEC amendment A9).

## [0.1.0] — 2026-09-27

First tagged release. Audio capture and local speech-to-text are built and
tested against real hardware; the app shell renders over them. Calendar,
Claude Code analysis and the MCP server are not in this release — see
"Not in this release" below.

### Added

**Audio capture (`crates/audio`, SPEC §2.2)**

- `AudioSource` trait with the macOS implementations behind it, keeping the
  Windows seam open from day one (SPEC §8.2).
- `MicSource` — real microphone capture through cpal, resampled to 16 kHz and
  written straight to WAV. Verified closed-loop against real hardware.
- `SystemSource` — system-audio capture via a Core Audio process tap, ported
  from the Phase 0a Swift spike. Verified against real hardware.
- Device-rate → 16 kHz resampler shared by both capture paths.
- Crash-safe incremental WAV writer, with checkpointing split into an fsync
  step and a header-patch step so a `kill -9` leaves a playable file.
- `segments.json` reader and writer — the on-disk drift contract (SPEC
  amendment A5), plus 18 committed fixtures covering drift, refusal and
  mid-recording device switches.
- A recording whose anchors stop before its audio does is refused rather than
  silently accepted.
- `meet-rec` CLI — orchestrates mic and system-audio capture end to end with
  no Tauri and no UI.

**Speech-to-text (`crates/stt`, `sidecar/meet-stt`, SPEC §2.6)**

- `SttEngine` trait with two implementations: Apple's on-device
  `SpeechTranscriber` via the Swift sidecar, and a whisper.cpp fallback.
- Live session seam — chunked transcription over audio that is still
  arriving, with the VAD state machine split out so live and batch paths
  agree on where an utterance ends.
- Silence gate that holds on the streaming path, guarding against whisper's
  hallucination-on-silence failure (SPEC §7, Phase 1 gate).
- `live_replay` example — replays a recorded meeting through the live session
  API as NDJSON, so UI work does not need a microphone or a model.
- Engine probe (`just stt-probe`) that decides offline between the Apple
  engine and the whisper fallback.

**Model download (`crates/modelfetch`)**

- Lazy, resumable, SHA-256-verified downloader for the pinned whisper models,
  with a `meet-stt-model` CLI (`just models`, `just model`).

**App shell (`src/`, `src-tauri/`, SPEC §2.7)**

- Tauri 2 + React 19 shell over the Rust workspace: sidebar, meetings list,
  review view, notes pane and record control.
- `/onboarding` route with the three steps SPEC §8.1 requires — permission,
  model download, meetings folder — including the denial path with a retry
  button and a System Settings deep link.
- Menu bar tray with rasters that invert correctly in light and dark.
- Typed IPC layer with a shared error taxonomy between Rust and TypeScript.
- Design-system tokens and app stylesheet carrying the meet-ai identity.

**Permissions and signing (SPEC §2.9, §8.1)**

- Positive-control permission check: a ~200 ms tone is played from meet-ai's
  own process and confirmed back through the tap. Return codes and RMS floors
  are both proven not to work on a denied tap (FINDINGS §10.1–10.2, §10.7).
- `just sign` — inside-out bundle signing that never uses `--deep`, gives
  nested helpers no entitlements, and fails loudly when a declared
  `externalBin` is missing from the bundle.
- Signing identity reads from `$SIGN_IDENTITY`, so a Developer ID build in v2
  is one environment variable and no code change.
- Bundle identifier fixed at `pro.saleschat.meetai` and the updater public key
  compiled in with `active: false` — both irreversible after the first
  install, both done now.

**Repo**

- `just check` as the single health gate: rustfmt, clippy with `-D warnings`,
  the full cargo test suite, the Windows cross-check seam, biome, tsc and
  vitest.
- Apache-2.0 license with a NOTICE file.

### Fixed

- The app window painted `transparent` because `--surface-canvas` was
  referenced but never defined. It is now an opaque token in both themes; the
  native material still shapes corners, shadow and active-state dimming, but
  is no longer asked to carry body-text contrast.
- App icon no longer nests the tile on macOS 26 — the 16pt and 32pt `.icns`
  representations are dropped, with a proxied legacy downscale for older
  systems.
- Traffic lights sit inside the window via `titleBarStyle: "Overlay"`.
- `NSUUID` crash in the system-audio tap.
- Nested helper binaries no longer inherit the app's microphone entitlement.
- Bundle signing no longer leaves a half-signed bundle that passes
  `codesign --verify --deep`.

### Changed

- macOS floor raised to 26.0 (SPEC amendment A8).
- Capture resolved to in-process Rust rather than the Swift sidecar SPEC §5
  had pre-committed to (SPEC amendment A6).

### Not in this release

Calendar integration, Claude Code analysis, ticket generation, the MCP server,
notarization, and Windows support. See SPEC.md §7 for the phase order and §8
for the seams each of those lands on.

[unreleased]: https://github.com/shantanujumde/meet-ai-app/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/shantanujumde/meet-ai-app/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/shantanujumde/meet-ai-app/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shantanujumde/meet-ai-app/releases/tag/v0.1.0
