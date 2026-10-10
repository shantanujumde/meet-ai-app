# Manual checks: tur159

TUR-159: model downloads. The fix:

- Whisper URLs are pinned to `ggerganov/whisper.cpp` commit
  `5359861c739e955e79d9a303bcbc70fb988958b1` (`stt::model::WHISPER_COMMIT`).
  On 2026-10-10 the Hugging Face API's `paths-info` at that commit gave the
  catalogue's size and LFS sha256 for all four files, so no entry is left
  unpinned. No model file was downloaded.
- After verification the digest is written beside the model as
  `<filename>.sha256`. A model whose digest file names another digest reads as
  not installed and is fetched again; one with no digest file (downloaded
  before this change) counts as installed and gets one written. Delete removes
  it with the model.
- A resume answered with a `Content-Range` that does not start at the resume
  offset (or none) deletes the `.part` and downloads from the start.
- Only transport errors, stalls, short bodies, 5xx and 429 are retried. A 404,
  403, 416, a full disk or a cancel is reported at once.
- `cancel_model_download` and a Cancel button on a downloading row (whisper
  rows and the Parakeet engine row). A cancelled download keeps its `.part`
  for a resume, and its thread drops the folder guard as it ends.
- Progress reaches the window when the whole percentage changes, or every
  100 ms, instead of per network chunk.
- `meet-stt-model list` lists Parakeet and `get parakeet-tdt-0.6b-v3` fetches
  the folder.
- Download state lives in a module store (`src/ui/engine/downloadStore.ts`),
  so leaving Settings or the onboarding speech step mid-download and coming
  back shows the row as downloading.

Ran headless here: `cargo test -p modelfetch -p stt -p meet-ai`, clippy on
the same three, `cargo fmt --all --check`, `pnpm vitest run src/ui/engine
src/ipc`, `pnpm typecheck`, biome on the touched files, the quality gate.
The modelfetch tests use a local plain-HTTP server (wrong `Content-Range`,
404 with no retry, 503 retried, cancel mid-body, throttled progress).
`cargo check --target x86_64-pc-windows-msvc -p stt` does not build on this
Mac (`onig_sys` needs a Windows C toolchain), so Windows is left to the CI
`rust (windows)` job.

Nothing below ran: no app was launched and nothing was downloaded.

## Run by hand

Use a signed build of this branch.

1. **Pinned download works.** Settings, Speech: download Small (English
   only). Expected: it finishes and verifies, and
   `<meetings>/.app/models/ggml-small.en-q5_1.bin.sha256` holds
   `bfdff489...ad30`. Skipped here: downloads a 190 MB model.
2. **Cancel.** Start Large (multilingual), press Cancel after a few percent.
   Expected: within a second the row shows Download again with no error, the
   `.part` file stays, and Settings, General can change the meetings folder
   (it said "still writing" before). Download again: it resumes from the
   kept bytes (the bar starts where it stopped). Skipped: needs the app and
   a 1 GB download.
3. **Leave and come back.** Start a download, open another settings tab or
   the meetings list, come back. Expected: the row still says Downloading…
   with the bar, and has no enabled Download button. Repeat from the
   onboarding speech step. Skipped: needs the running app.
4. **Parakeet.** Start and cancel the Parakeet download on the engine row.
   Expected: same as 2.
5. **CLI.** `cargo run -p modelfetch --bin meet-stt-model -- list` shows
   `parakeet-tdt-0.6b-v3`; `get parakeet-tdt-0.6b-v3 --dir <scratch>` fills
   `<scratch>/parakeet-tdt-0.6b-v3-int8/`. Skipped: `get` downloads 670 MB.
6. **Fewer events.** With the web inspector open during a download, count
   `model://progress` events. Expected: about one per percent plus up to ten
   a second, not thousands. Skipped: needs the running app.
7. **An old install.** On a Mac with a model downloaded by an earlier
   version (no `.sha256` beside it), open Settings. Expected: the model is
   still "On this Mac", is not downloaded again, and a whisper recording
   uses it. Skipped: needs an earlier install.
