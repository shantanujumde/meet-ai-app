# TUR-111: Review transcript in a scrolling box

Skipped here: jsdom has no layout, so scrolling cannot be seen headless.

1. Open a past meeting with 100+ transcript lines in the running app.
   Expected: the transcript sits in a box at most min(24rem, 45vh) tall that scrolls
   on its own; the Tasks and Notes sections below are visible without scrolling past it.
2. Tab to the box, press arrow keys, Page Up, Page Down.
   Expected: the box scrolls; a focus ring shows.
3. Open a meeting with about 5 lines.
   Expected: looks the same as before, with no extra empty space.
4. Heading, read-only hint and the unreadable-lines warning stay outside the box.
