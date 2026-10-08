# Manual checks: TUR-116 (meeting row ⋯ menu, right-click menu, Delete)

The item list (`src/ui/meetingMenu.ts`), both menus opening with the same
items, Delete's cancel and confirm paths, the recording block and the inline
rename are covered by vitest (`src/ui/meetingMenu.test.ts`,
`src/ui/MeetingRowMenu.test.tsx`). The Rust checks in front of the Trash
(`delete_in`: recording, bad ids, the shared tickets folder, a missing folder)
are covered by `cargo test -p meet-ai --lib meetings::delete`. Nothing below
was run here: each needs the running, signed app (and for 6 to 8, Windows or
Linux).

## Run by hand

1. Right-click a meeting in the sidebar's Recent meetings, and in the
   Meetings list. Also Control-click.
   Expect: the meeting menu (Open, Show in Finder, Copy folder path, the notes
   items, Copy transcript, Rename…, Delete…), never the system text menu (Look
   Up, Translate, Services). It opens at the mouse.
   Why skipped: jsdom fires the event but has no native menu to suppress.
2. Hover a row, Tab to a row, and open a meeting.
   Expect: the ⋯ button shows on hover, on keyboard focus, and always on the
   selected row (white on the accent pill). It never overlaps the row's text.
   Why skipped: jsdom has no layout or hover.
3. Focus a row with Tab and press Shift+F10 (and the context-menu key on a
   keyboard that has one). Then arrow keys, Enter, Escape.
   Expect: the ⋯ menu opens with the first item focused; arrows move, Enter
   picks, Escape closes and focus goes back to the ⋯ button. Verify it: if
   WebKit also fires a `contextmenu` event for these keys, only one menu
   should open.
   Why skipped: needs the real webview's key handling.
4. Delete… on a finished meeting, Cancel; then Delete… again, Delete.
   Expect: a warning dialog "Delete “<title>”?" / "Its transcript, notes and
   audio will be moved to the Trash." (plus "Tickets made from it stay in
   Tickets." when a ticket in Tickets names it). Cancel changes nothing. Delete
   removes the row from the sidebar and the Meetings list; with the meeting
   open, the window goes to Meetings. The folder is in the Trash, whole, and
   the shared ticket is still in Tickets.
   Why skipped: needs the signed app and a real meetings folder.
5. In Finder, open the Trash and look at the deleted folder.
   Expect: it is there. The delete uses `NSFileManager` (no "meet-ai wants to
   control Finder" prompt, ever). Verify it: on some macOS versions "Put Back"
   is missing for items trashed this way; dragging the folder back into the
   meetings folder must then bring the meeting back in the list.
   Why skipped: needs a real Trash.
6. Start a recording, open its row's menu.
   Expect: only Open and Show in Finder. There is no Delete for it.
   Why skipped: needs a real recording.
7. Windows: Delete… on a meeting.
   Expect: the dialog says "Recycle Bin", the folder lands in the Recycle Bin,
   and the menu item says "Show in File Explorer".
   Why skipped: needs Windows; the CI job only builds and tests.
8. Linux: Delete… on a meeting.
   Expect: the folder lands in `~/.local/share/Trash` (or the drive's
   `.Trash-<uid>`), and the file manager's Trash shows it.
   Why skipped: needs a Linux desktop.
9. Rename… from a row's menu.
   Expect: the title turns into a text field in place, all selected. Enter
   saves (the meeting page shows the new name too), Escape keeps the old one,
   clicking away saves. Right-clicking inside the field shows the system's
   text menu (cut, copy, paste), not the meeting menu.
   Why skipped: needs the running app.
10. Write notes now, Stop writing notes, Copy prompt (with the agent set to
    none), Turn notes off / on, Copy transcript, Copy notes, Copy folder path,
    Show in Finder, each from the menu.
    Expect: each does what the same control on the meeting page does; the
    clipboard holds the transcript in `[HH:MM:SS] You: …` lines.
    Why skipped: needs the running app and an agent CLI.

## Known

- A run's agent is cancelled before its folder goes to the Trash, but the
  cancel is a signal: if the agent is just finishing a write, that write fails
  (the folder is gone) and the run ends as failed or cancelled. Nothing is
  written outside the Trash.
- The shared `tickets` folder sits beside the meetings, and `store::folder::scan`
  lists it like a meeting folder. Delete refuses that id (`not-a-meeting`), so
  the shared tickets cannot be trashed from a row.
