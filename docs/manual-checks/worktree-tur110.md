# Manual checks: TUR-110 (meeting page scrolls into empty space)

Skipped here: needs the running signed app; jsdom has no layout.

1. Start a recording and let 20+ live lines arrive. Open the meeting page and scroll the content pane to the bottom.
   Expected: scrolling stops at Notes plus the normal bottom padding; no empty screen below.
2. At the right edge of the content pane, look for clipped glyphs or a small gray bar.
   Expected: nothing drawn outside the page column.
3. Stop recording, reopen the same meeting (not recording) and repeat 1 and 2. Also scroll Meetings, Tickets and Settings to the bottom.
   Expected: each stops at its content.
4. With VoiceOver on, new live lines are still announced with "You:" / "Others:".
