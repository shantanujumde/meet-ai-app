# Manual checks: TUR-86 (Join links: the backslash trick, and links pasted into cloud events)

These need the running, signed app on a real Mac (and a Windows desktop for
§3) with a calendar connected, so they were not run here: an agent run cannot
open the app, sign in to Google or Microsoft, or click the menu bar.

The headless parts are covered by:

- `cargo test -p calendar --lib join_url`: `is_safe_join_url` refuses a `\`
  before the path (`https://evil.example\@meet.google.com/…`,
  `https:\\evil.example/…`), user info in either direction
  (`https://meet.google.com@evil.example/…`, `https://user@meet.google.com/…`),
  look-alike hosts, a host hidden behind `#` or `?`, non-http(s) schemes and
  unparsable text; it accepts mixed-case hosts, ports, and a `\` inside the
  query. `extract_join_url` uses the same check.
- `cargo test -p calendar --test google_provider`: a Zoom link in a Google
  event's `location`, and in its HTML `description`, becomes the Join link;
  phishing `hangoutLink`/`conferenceData` links are dropped and the location
  is used instead; `fields` asks for `location,description`.
- `cargo test -p calendar --test microsoft`: a Zoom link in an Outlook
  event's `bodyPreview` and a bare Meet link in `location.displayName`
  become the Join link; a phishing `onlineMeeting.joinUrl` is dropped;
  `$select` has `location,bodyPreview`.
- `cargo test -p meet-ai --lib tray::menu_model`: an event whose link fails
  the check gets no Join item. `tray/today.rs` runs the same check right
  before `open_url`.

## 1. A pasted Zoom link gets Join from a cloud calendar (macOS)

1. `just bundle-signed`, open meet-ai, connect Google (Settings → Calendar)
   on a Mac whose Calendar app has no Google account.
2. In Google Calendar make an event for later today, two attendees, no Meet
   conference, and paste `https://zoom.us/j/1234567890` into Location.
3. Wait for the next calendar read (or reopen the menu).

Expected: the event's submenu in the menu bar has **Join**, and it opens the
Zoom link. Repeat with the link in the description instead, and with an
Outlook event (Microsoft connected) with the link in the body.

## 2. The backslash link gets no Join (macOS)

1. As above, an event whose Location is
   `https://evil.example\@meet.google.com/abc-defg-hij`.

Expected: no **Join** in that event's submenu. Then also check what macOS
does with the raw string, since the ticket asks for it: in Terminal,
`open 'https://evil.example\@meet.google.com/abc-defg-hij'` and note which
host the browser lands on (NSURL may percent-encode the `\`). Whatever it
does, meet-ai no longer offers the link, so this is information only.

## 3. Windows

Same as §1 and §2 on Windows with Microsoft connected, once the Windows build
has the tray. ShellExecute hands the string to the browser as written, which
would open `evil.example`: the check must keep it from ever reaching it.

## Behaviour change to know about

A conference link from Google (`hangoutLink`, `conferenceData` video entry)
or Microsoft (`onlineMeeting.joinUrl`) on a host outside the allow-list
(Zoom, Meet, Teams, Webex, Slack huddles) no longer gets **Join**. Before,
any http(s) link from those fields did. That is what "no link that a
browser would open on a non-allow-listed host is ever offered as Join"
requires; an add-on service people use (say Whereby) would need adding to
`SERVICES` in `crates/calendar/src/join_url.rs`.
