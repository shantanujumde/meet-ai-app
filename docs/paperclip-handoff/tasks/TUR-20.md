# TUR-20 — check why these windows are appearing

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 13:19 UTC by You (board) |
| Completed | 2026-09-27 13:23 UTC |

## Description

![](/api/assets/35cee0d6-8dbd-48c4-9af7-0695ba7c1d94/content)

## Comments (1)

### Alen · 2026-09-27 13:23 UTC

> ## Why those windows appear
>
> They are the brand render scripts launching headless Chrome. Each launch blocks on a macOS keychain dialog.
>
> **The chain:**
>
> 1. `render.sh` and `_shoot.zsh` start Chrome with `--user-data-dir=$(mktemp -d)` — a brand new, empty Chrome profile every single time.
> 2. On startup Chrome always tries to save its "Chrome Safe Storage" key (the secret it encrypts saved passwords with) into the Mac's default keychain. Because the profile is new, there is never an existing key to reuse, so it attempts that write on *every* launch.
> 3. The scripts run under a sandboxed `$HOME` (the Paperclip runner sets `HOME` to a temp folder). That folder has no `Library/Keychains` in it, so macOS reports there is no keychain at all. Confirmed directly:
>
>    ```
>    $ security default-keychain
>    security: SecKeychainCopyDefault: A default keychain could not be found.
>    ```
>
> 4. macOS then puts up the modal panel in the screenshot — *"A keychain cannot be found to store 'Chrome'"* — and waits. **`--headless` does not suppress it.**
> 5. Chrome sits on that dialog and never writes the screenshot. The script's wait loop times out, retries, and each retry raises another dialog. Both scripts retry **5 times per image**, which is why they stack up.
>
> ### Your actual keychain is fine
>
> Worth saying plainly, because it looks alarming: nothing is broken on your Mac. Read with your real home directory, everything is healthy — `login.keychain-db` is present, it is the default, and `meet-ai-signing.keychain-db` is correctly on the search list. `make-identity.sh` did its job. The dialog is purely an artifact of the redirected `$HOME`.
>
> ### This also explains the "startup race"
>
> `render.sh` carries a comment about Chrome "losing the startup race intermittently and exiting without writing anything," which is what the retry loop was built for. That intermittent failure is largely this dialog. It looked random because it depended on whether a dialog was already up.
>
> ## Fix applied
>
> Added `--use-mock-keychain` to both Chrome invocations — it tells Chrome to keep the storage key in memory instead of the keychain. Nothing is saved between runs anyway, since the profile is a temp dir that gets deleted.
>
> ```
>  design-system/meet-ai/brand/tools/_shoot.zsh | 11 +++++++++++
>  design-system/meet-ai/brand/tools/render.sh  | 13 +++++++++++++
> ```
>
> `render-proof.sh` and `render-proximity.sh` both `source ./_shoot.zsh`, so all four renderers are covered.
>
> ### Verified
>
> Ran the edited `shoot_page` under the same sandboxed `$HOME`, on a real stage input:
>
> ```
> verify-icon.png   512x512  ink 58%
> shoot_page rc=0   elapsed=2s   (succeeded first attempt, 0 retries)
> SecurityAgent processes spawned: 0
> ```
>
> Before, a launch would hang until the 20s wait loop gave up. Now it renders in ~1-2s with no prompt. Both scripts pass `zsh -n`.
>
> ## The Finder window — separate, not a bug
>
> The empty Finder window is `target/release/bundle/macos`, the Tauri bundle output folder. It is genuinely empty (created 18:46, 0 items), so a bundle step produced nothing. Unrelated to the keychain dialogs — flagging it in case that empty output is itself unexpected.
