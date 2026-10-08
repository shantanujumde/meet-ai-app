# Manual checks: tur147

TUR-147: every prompt shows in meet-ai's own card window on every OS, macOS
included (`popup::uses_popup` is true for every signal; SPEC A27). Calendar
reminders keep TUR-108's 380×72 card. Detection prompts get a narrow
220×120 card (`src/ui/promptCards.tsx` `StartCard`): our app icon, one line
("Zoom call", "Audio activity"), **Record**, **Not now**, and "⋯" with
**Never for Zoom** in a native menu. A third card, the countdown
(`CountdownCard`, `detection/popup/countdown.rs`): "Zoom call ended", a ring
counting down, **Stop now**, **Keep recording**. On macOS the window is
turned into a non-activating `NSPanel` that joins every Space and shows over
full-screen apps, and is shown with `orderFrontRegardless`
(`detection/popup/platform/macos.rs`). Cards slide in and fade out, not with
Reduce Motion.

Tested headless on a Mac:

- `cargo test -p meet-ai --lib detection::` (93 tests): prompt `headline`
  and `app`, every answer including `NeverFor`, `StopNow`, `KeepRecording`,
  how a countdown ends (button, time out, replaced, cancelled), the window
  sizes and top-right maths with and without the shadow inset, and on macOS
  that the panel class registers, answers `canBecomeKeyWindow` /
  `canBecomeMainWindow`, has tao's `focusable` ivar and the same instance
  size as an `NSWindow` subclass with that ivar (so the class swap happens on
  this macOS), and the collection behaviour.
- `pnpm vitest run src/ui/PromptPopup.test.tsx src/ui/DetectionPrompt.test.tsx`:
  all three cards, each button's answer, the "⋯ Never for" menu, no "⋯"
  for audio activity, the countdown ticking (fake clock), a late-loading
  window, the fade-out with and without Reduce Motion (stubbed
  `matchMedia`), Escape, and the recording state closing the right card.
- The Tailwind classes the cards use (`motion-safe:starting:*`,
  `[[data-os=macos]_&]:*`, `stroke-*`, `duration-(--dur-*)`) were compiled
  with `@tailwindcss/node` against `src/index.css` and each produced CSS.

Nothing below ran: no app was launched, and no window was made.

## Run by hand (Wave H)

Use a signed build of this branch on macOS 26, and builds on Windows 11,
Linux X11 and GNOME Wayland.

1. macOS: start a Zoom meeting, make it full screen, and talk in it (or type
   in its chat) with meet-ai idle. Wait for the prompt (the detection rules
   are unchanged by this ticket: Zoom open, or mic and speakers in use).
   Expect: the narrow card appears top-right over the full-screen Zoom call,
   just under where the menu bar is. Zoom keeps the keyboard (keep typing in
   its chat: the letters go to Zoom), meet-ai does not become the active app
   (the menu bar still says zoom.us), and no Dock bounce.
   Why skipped: needs a real meeting app, a full-screen Space and the signed
   app. Verify it: the class swap happened (log line `the prompt card is a
   non-activating panel`; a `stays a plain window` warning means it did
   not), and the card really shows over another app's full-screen Space with
   meet-ai's regular (Dock) activation policy.
2. macOS: with the card up, switch desktops (Ctrl-→, or Mission Control).
   Expect: the card is on every desktop, and not in ⌘\` window cycling.
   Why skipped: needs the running app.
3. macOS: click **Record** on the card while Zoom is the active app.
   Expect: one click records (no click just to focus), meet-ai is still not
   the active app, and after the card goes the keyboard is back in Zoom.
   Then a new prompt: click **Not now**: it closes and nothing records.
   Why skipped: needs the running app. Verify it: whether a WKWebView click
   makes the panel key (`becomesKeyOnlyIfNeeded`); if it does, the keyboard
   must still go back to Zoom when the card hides.
4. Click "⋯".
   Expect: a native menu under it with **Never for Zoom**; choosing it
   closes the card and records nothing (TUR-143 then stores the list; until
   it merges the log says `Never for: not asking about this app now`). With
   the prompt from mic-and-speakers activity ("Audio activity") there is no
   "⋯".
   Why skipped: needs the running app.
5. VoiceOver (⌘F5) with the card up: VO-F2 or the window chooser to reach
   it, then move through it.
   Expect: VoiceOver reads "Zoom call", then **Record Zoom call**, **Not
   now, do not record**, **More choices**, and each one presses with VO-Space.
   Escape, once the card has the keyboard, is Not now.
   Why skipped: needs the running app and VoiceOver.
6. Leave the prompt alone.
   Expect: after about 20 s it fades out and goes; nothing records. With
   System Settings, Accessibility, Display, **Reduce motion** on: it appears
   and goes with no slide or fade.
   Why skipped: needs the running app.
7. Windows 11 and Linux X11: the same prompt.
   Expect: the same narrow card top-right of the primary screen, with a soft
   shadow around it inside the window (the window is 8 px bigger on each
   side) and transparent corners; no taskbar entry, no focus taken. GNOME
   Wayland: the same card, placed wherever the compositor puts it.
   Why skipped: needs those OSes. Verify it: the transparent margin shows
   the desktop, not a black or white box (no compositor on some Linux X11
   setups), and the card is visible on every workspace
   (`visible_on_all_workspaces`).
8. Reminder card regression: Settings, Notifications, **Send a test
   reminder**.
   Expect: TUR-108's 380×72 card, unchanged, now also sliding in and fading
   out; on macOS also over full-screen apps and on every desktop.
   Why skipped: needs the running app.
9. Countdown card: once TUR-144 (the first caller) is merged, record a Zoom
   call and hang up.
   Expect: "Zoom call ended", a ring counting 10 to 0 with the number in it,
   **Stop now** and **Keep recording**. It does not go after 20 s; at 0 it
   closes (TUR-144 stops). Stop now and Keep recording close it at once.
   Same panel behaviour as checks 1 to 3. With VoiceOver the ring reads
   "Stopping in N seconds" and is not announced every second.
   Why skipped: nothing calls `show_countdown` until TUR-144/TUR-145.
10. Design review with the `impeccable` skill, light and dark, glass on and
    off. Screenshots to capture (each on macOS and Windows; one on Linux):
    the start card for "Zoom call"; the start card for "Audio activity" (no
    "⋯"); the "⋯" menu open; the countdown card at 10 and at about 3
    seconds; the reminder card next to the start card, to compare.
    Expect: the cards follow the TUR-102/TUR-103 look (popup surface, rim,
    radius, type scale, one accent button), the icon and line align, the
    button labels fit at 220 px, and long app names truncate with an ellipsis.
    Why skipped: needs the running app and screenshots.

## Known limits

- The class swap to `NSPanel` relies on the panel subclass having the same
  instance layout as tao's `TaoWindow` (`NSWindow` plus one BOOL); a unit
  test checks it on the build Mac's AppKit, and the app checks again at
  runtime and falls back to a plain floating window if a future macOS or tao
  changes it.
- `_setPreventsActivation:` is private AppKit; it is called only if the panel
  answers to it (as tauri-nspanel does).
- A countdown that is cancelled from Rust (`CountdownHandle::cancel`) hides
  without the fade, because the window is not told first.
