# TUR-19 — check why these windows are opening up?

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 13:18 UTC by You (board) |
| Completed | 2026-09-28 06:09 UTC |

## Description

![](/api/assets/bf8e28a6-aa96-49c8-8951-c99d9047d7b3/content)

## Comments (1)

### Alen · 2026-09-28 06:09 UTC

> ## Why those windows opened
>
> They are macOS **"Keychain Not Found — A keychain cannot be found to store 'Chrome'"** dialogs. Nothing was wrong with your keychain file. Your **login keychain fell off the keychain search list**, so every Chromium-based app (Chrome, Helium, and the Electron apps) could no longer reach its saved `… Safe Storage` password and re-asked — one dialog per attempt, which is why they stack.
>
> ### What knocked it off
>
> Our own signing script. `spikes/phase0a-tcc/make-identity.sh` puts the signing keychain on the search list with:
>
> ```
> security list-keychains -d user -s $CURRENT "$KEYCHAIN"
> ```
>
> `-s` **replaces** the list rather than appending. When that runs inside an agent shell, `$HOME` is redirected to a sandbox temp directory, `security` reads the preferences from there, and `$CURRENT` comes back **empty** — so the line rewrites your real search list with the signing keychain as the only entry. `login.keychain` is gone from it, and the dialogs start.
>
> Verified: the `Chrome Safe Storage` item does live in `login.keychain-db`, so losing that keychain from the list is exactly what triggers the prompt.
>
> ### Timeline
>
> | When | What |
> |---|---|
> | 2026-09-27 13:18 | you filed this issue with the screenshot |
> | 2026-09-27 13:57 | commit `d2b79c7` (TUR-10) added the guard — the script now refuses to write an empty list and prints a warning instead |
> | 2026-09-28 11:16–11:17 | `--rotate` re-ran correctly and rewrote the search list properly |
>
> ### Current state — healthy
>
> ```
> $ security list-keychains
>     /Users/shantanujumde/Library/Keychains/login.keychain-db
>     /Users/shantanujumde/Library/Keychains/meet-ai-signing.keychain-db
>     /Library/Keychains/System.keychain
> $ security default-keychain
>     /Users/shantanujumde/Library/Keychains/login.keychain-db
> ```
>
> No dialogs are pending and no `SecurityAgent` process is running. Root cause is already fixed in the repo, so this should not recur from the signing flow.
>
> ### If it ever comes back
>
> Click **Cancel**, not *Reset To Defaults* (that button's behaviour varies and can leave you with an empty keychain). Then, in a **normal Terminal**:
>
> ```sh
> security list-keychains -d user -s \
>   ~/Library/Keychains/login.keychain-db \
>   ~/Library/Keychains/meet-ai-signing.keychain-db
> security default-keychain -s ~/Library/Keychains/login.keychain-db
> ```
>
> Committed that as a troubleshooting section in `RELEASING.md` (`6c25f00`) on `docs/releasing-guide`, so the next person who hits it has the recovery to hand.
>
> ### One note on your passwords
>
> No saved passwords were lost — the keychain was only unreachable, never erased. But Chrome may have been unable to decrypt saved logins while the dialogs were up; if any site logins look forgotten, they should come back now that the list is restored.
