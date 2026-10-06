# TUR-114: Meetings list rows left-aligned

## Row text starts at the left edge

- Run: open the app, go to Meetings with several meetings of different title lengths.
- Expected: each row's name, attendee line and date line start at the same left edge right after the icon; the right side (lines, pills) stays right-aligned.
- Why skipped: jsdom has no layout; the unit test only checks the `text-start` class. Needs the running app.
