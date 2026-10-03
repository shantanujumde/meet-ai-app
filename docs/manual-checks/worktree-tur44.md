# Manual checks: TUR-44 (Sign in with Google / Microsoft: PKCE loopback, token in the OS keystore)

These need real Google and Microsoft accounts, a browser, the real OS keystore
(Keychain, Windows Credential Manager, Secret Service) and the running app on
each OS, so they were not run here: an agent run has no accounts, cannot click
through a consent screen, and must not write to the real keychain. There is no
UI yet (TUR-49 builds the Settings card), so drive the commands from the
webview's devtools console with `window.__TAURI_INTERNALS__.invoke(...)`.

The headless parts are covered by:

- `cargo test -p calendar oauth`: the authorize URL (S256 challenge, 32-byte
  base64url `state`, loopback redirect, scopes, Google's `access_type=offline`
  and `prompt=consent`); a `state` mismatch, a missing `state` and a forged
  `access_denied` are all `Failed`, with no code exchanged and nothing stored;
  the code exchange sends the verifier (and Google's non-secret client secret,
  Microsoft none); only the refresh token is stored; silent refresh, a rotated
  refresh token kept, a rejected refresh token is `SignInExpired` and the
  account `Expired`; unreachable is not expired; a restart signs in silently;
  no keystore keeps the sign-in in memory (`remembered: false`); sign-out.
  All against a fake token endpoint and an in-memory store.
- `cargo test -p meet-ai --lib calendar::signin`: the real loopback listener
  (`tauri-plugin-oauth`) on a random 127.0.0.1 port, fed the way the plugin's
  page script feeds it: the right `state` signs in, a local process with the
  wrong `state` is `calendar-sign-in-failed` with nothing stored, no reply is
  `calendar-sign-in-cancelled`, no client id is `calendar-not-configured`
  naming the key and opens no browser, and the wire shape.
- `cargo test -p meet-ai --lib config::calendar_section`: the new
  `calendar.google` / `calendar.microsoft` keys, `"ics"` now an unknown
  provider, old `ics_urls` still loading.

Before any check: set up the clients as in SETUP.md, "Calendar sign-in
(optional)", and paste the ids into `~/Meetings/.app/config.jsonc`.

## 1. Sign in, on each of macOS, Windows and Linux

1. Start a dev build. In devtools run
   `await window.__TAURI_INTERNALS__.invoke("calendar_sign_in", { provider: "google" })`.
2. Expected: the default browser opens Google's consent screen. The address
   bar's `redirect_uri` is `http://127.0.0.1:<port>/callback` and the URL has
   `code_challenge_method=S256`. Approve.
3. Expected: the tab shows "Signed in. You can close this tab and go back to
   meet-ai." and the command answers
   `{ provider: "google", account: "<your email>", state: "signed-in", remembered: true }`.
4. Repeat with `provider: "microsoft"`, once with a personal account and once
   with a work account. Expected: the same, with `account` your username.
5. Check the keystore holds only the refresh token: macOS Keychain Access,
   item `pro.saleschat.meetai` / `calendar-google`; Windows Credential
   Manager → Windows Credentials; Linux `secret-tool search service
   pro.saleschat.meetai`. The secret is a single token string, not JSON.

## 2. Restart and silent refresh

1. Quit the app fully and start it again. Run `calendar_accounts`.
2. Expected: both providers `signed-in` with the account shown, with no
   browser opening.
3. Leave the app running for over an hour (an access token's life), then have
   something call `CalendarAuth::access_token` (TUR-47/48's providers; until
   they land, `calendar_accounts` after a restart exercises the same refresh).
   Expected: no browser, no error. For Microsoft the keystore item's value
   changes (refresh tokens rotate).
4. The Phase 5b gate (SPEC §6): wait a day (Google: past 7 days too, which is
   what catches an OAuth app left in *Testing*), relaunch, still signed in.

## 3. Cancel, deny, expire, sign out

1. Start `calendar_sign_in` and close the tab without approving. After 5
   minutes, expected: error kind `calendar-sign-in-cancelled`, and the port no
   longer answers (`curl http://127.0.0.1:<port>/` fails).
2. Start it again and press **Cancel** / deny on the consent screen.
   Expected: `calendar-sign-in-cancelled` at once.
3. While a sign-in waits, run
   `curl -H "Full-Url: http://127.0.0.1:<port>/callback?state=x&code=y" http://127.0.0.1:<port>/cb`.
   Expected: the command answers `calendar-sign-in-failed` and nothing is
   stored (the listener stops after one reply, so start the sign-in again).
4. Revoke the app's access (Google: myaccount.google.com → Security → Third
   party access; Microsoft: account.live.com/consent/Manage or
   myapps.microsoft.com). Restart and run `calendar_accounts`. Expected:
   `state: "expired"` for that provider.
5. Run `calendar_sign_out` for each provider. Expected: `signed-out`, and the
   keystore item is gone.
6. Remove `calendar.google.client_id` from `config.jsonc` and sign in.
   Expected: `calendar-not-configured`, message naming
   `calendar.google.client_id`, no browser.

## 4. Linux with no Secret Service

1. On a Linux session with no gnome-keyring or KWallet running, sign in.
2. Expected: sign-in succeeds with `remembered: false`, and the log has one
   warning starting "sign-in won't be remembered: no Secret Service
   (gnome-keyring or KWallet) running". After a restart, `signed-out`.

## 5. macOS keychain prompt in a dev build

An unsigned or differently signed dev build may make macOS ask "meet-ai wants
to use your confidential information stored in … in your keychain" on the
first read after a rebuild. Check a signed build (`just bundle-signed`) does
not ask after the first **Always Allow**.

## Notes for reviewers

- The loopback listener (`tauri-plugin-oauth` 2.1.0) reads each request with a
  single `read` of at most 4048 bytes. Browsers send the page script's `fetch`
  in one segment, so this works, but the headless test had to send its fake
  request in one write too. If a real browser ever fails step 1.3, look here.
- The callback page says "Signed in." as soon as the browser lands, before the
  app has checked `state` and exchanged the code; the plugin serves a static
  page. A failed exchange still shows that page; the command's answer is what
  counts.
