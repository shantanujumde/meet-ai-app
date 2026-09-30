# TUR-68 — Phase 1c — prove the model download on a cold machine, including the interrupted one

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Vox |
| Created | 2026-09-28 04:11 UTC by Alen |
| Completed | 2026-09-28 04:22 UTC |
| Parent | [TUR-5](TUR-5.md) Phase 1 — transcription: Apple built-in + Whisper fallback |

## Description

TUR-5 asks for a lazy model download that is "resumable, checksum-verified, pinned source, clear progress, and a sane recovery when the download dies halfway". `crates/modelfetch` is written and its doc comment claims all of it. Nothing shows it has been exercised against the real network.

#### Scope

Run it, on a machine with no model on disk, and report what happened:

- **Cold fetch.** `just models` from nothing. Report size, duration, and that the SHA-256 matched the pin in `stt::model`.
- **Resume.** Kill the download partway, restart it, and confirm it continues from the `.part` file with a `Range` request instead of starting over. Report the byte offset it resumed from.
- **Corruption is caught.** Truncate or flip a byte in a `.part` file, let the download finish, and confirm the checksum mismatch stops it renaming into place. A file under its real name must always be a verified file.
- **Second run is free.** Confirm a finished model makes no request and is not re-hashed on every launch.
- **Progress is legible.** Whatever the user sees while 190 MB downloads, look at it and say whether it is good enough.

#### Done when

Each bullet has a real run behind it and a result posted here — actual output, not a reading of the code.

Repo: `/Users/shantanujumde/apps/meet-ai`, branch `main`. Parent: TUR-5.

## Comments (2)

### Vox · 2026-09-28 04:22 UTC

> All five TUR-68 checks run against the real network with real numbers posted above: cold fetch (190,098,681 bytes, 64.4s, checksum match), hard-kill resume (from byte 17,688,674), corruption caught and .part deleted rather than promoted, second run free (0.01s, mtime unchanged), progress bar assessed legible. No follow-up blocking this ticket.

### Vox · 2026-09-28 04:22 UTC

> Ran `crates/modelfetch` against the real Hugging Face CDN, on a machine with no model on disk (fresh `~/Meetings`). All five checks below are actual runs, not a reading of the code. Binary used: `cargo build --release -p modelfetch --bin meet-stt-model` (this is what `just model` runs), model `small.en-q5_1` (pinned in `stt::model::MODELS`: 190,098,681 bytes, sha256 `bfdff489...78ad30`).
>
> **1. Cold fetch**
> `./target/release/meet-stt-model get small.en-q5_1` into the default dir (`~/Meetings/.app/models/`), starting from nothing.
> - Size: 190,098,681 bytes (matches the pinned `bytes` exactly)
> - Duration: 1m 4.4s wall clock (~2.95 MB/s)
> - Checksum: `shasum -a 256` on the finished file = `bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30` — matches the pin in `stt::model` exactly.
> - Exit code 0, final line printed the path to the verified file.
>
> **2. Resume after a hard kill**
> Started a fresh `get` into a scratch dir, let it run 8s, then `kill -9` it (no graceful shutdown — the realistic "app died" case, not a clean Ctrl-C).
> - `.part` file on disk after the kill: **17,688,674 bytes** (~9%, 17.9 MB).
> - Restarted the identical command. First progress line printed was `9%  18/190 MB` — it picked up from the existing bytes, not from 0.
> - It fetched only the remaining ~172 MB in 56.2s (vs 64.4s for the full 190 MB cold fetch above — consistent with a partial transfer, not a redownload).
> - Final file's SHA-256 again matched the pinned digest exactly.
> - (Independently confirmed the CDN itself honors `Range`: a manual `curl -I` on the pinned URL returns `accept-ranges: bytes`.)
>
> **3. Corruption is caught**
> Copied the already-verified good file into a fresh scratch dir as `<filename>.part` (full pinned size, so `ensure()` skips straight to hashing) and flipped one bit at byte offset 1000. Ran `get` against it:
> - Output: `model small.en-q5_1 failed its checksum: expected bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30, got 829f98d015a6193b22dc7e452a5741cc5e324551a326801603fff1ead6cb78df`
> - Exit code 1.
> - Directory after: **empty**. The `.part` was deleted, nothing was renamed to the real filename. A file under the real name is always a verified file, confirmed.
>
> **4. Second run is free**
> Ran `get small.en-q5_1` again against the already-downloaded file.
> - Wall time: 0.010s total (vs 64s for the cold fetch).
> - No progress lines emitted at all (the download path never ran).
> - File mtime unchanged before/after (`1790569168`), so it wasn't rewritten or re-verified by copy.
> - Matches the crate's own unit test invariant ("the installed path must not report progress"), now confirmed against a real 190 MB file, not the 8-byte test fixture.
>
> **5. Progress legibility**
> The CLI redraws one line via `
> ` (checked the raw bytes of a captured run: 94 carriage returns vs 2 newlines) showing `NN%  current/total MB`, then switches to `verifying checksum...`, then a final `verified` line. Progress/status goes to stderr, the finished path goes to stdout on its own line, so scripting against stdout stays clean.
> **Verdict: good enough.** Percentage + running MB count is exactly what you want for a 190 MB wait, and stderr redraw means no scrollback spam in a real terminal (piping to a file/tool that doesn't do CR is the only way to see it as one long smear, which is what my logs above look like — not what a user sees).
> One small nit, not blocking: the final `verified` line prints unconditionally, including on the free/no-op path in check 4 — on that path nothing was actually re-verified this run, it's just confirming presence. Cosmetic; happy to open a follow-up if you want it worded differently (e.g. "verified" vs "already installed").
>
> All five bullets pass. I'm calling TUR-68 done — I don't see follow-up work this ticket is blocking on.
