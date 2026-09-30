# meet-ai

meet-ai is a meeting recorder for macOS that never joins the call as a bot.
It records your microphone and the other people's audio on your own Mac,
turns both into a transcript on the device, and keeps everything as plain
files in a folder you own. Nothing is uploaded.

It is built with Tauri 2: a Rust core with a React window. On macOS 26 and
later it uses Apple's built-in speech engine, so there is no model to
download. Whisper is there as a fallback.

**Status: v0.3.0.** Personal use only. Builds are signed with a local
self-signed identity, not a Developer ID, and are not notarized. See
[`CHANGELOG.md`](./CHANGELOG.md) for what has shipped.

## Quick start

You need macOS 26 or later, Xcode Command Line Tools, Rust (pinned by
`rust-toolchain.toml`), Node 20.19 or later, `just` and `cmake`.
[`CONTRIBUTING.md`](./CONTRIBUTING.md) has the full setup, including the
signing identity.

```bash
pnpm install
just check   # formatting, lint, types and every test. Green means healthy.
just dev     # run the app
```

## Documents

| File | What it is for |
|---|---|
| [`SPEC.md`](./SPEC.md) | The build contract: locked decisions and the phase plan. It wins over everything else. |
| [`SETUP.md`](./SETUP.md) | Every pinned dependency version, and why. |
| [`CONTRIBUTING.md`](./CONTRIBUTING.md) | How to set up, check and work on the repo. |
| [`RELEASING.md`](./RELEASING.md) | How to cut a release. |
| [`CHANGELOG.md`](./CHANGELOG.md) | What changed in each version. |
| [`docs/problem.md`](./docs/problem.md) | What the product is for, and who it is for. |
| [`docs/findings.md`](./docs/findings.md) | The research behind the stack choices. |
| [`docs/history/discovery-v1.md`](./docs/history/discovery-v1.md) | The first design doc (the old `Readme.md`). Superseded by `SPEC.md`; kept for the reasoning trail. |

## License

See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE).
