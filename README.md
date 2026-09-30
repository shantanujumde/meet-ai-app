<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="design-system/meet-ai/brand/meet-ai-logo-horizontal-chalk.svg">
    <img src="design-system/meet-ai/brand/meet-ai-logo-horizontal-ink.svg" alt="meet-ai" width="320">
  </picture>
</p>

<p align="center">
  A meeting recorder for macOS that never joins the call as a bot.<br>
  <a href="https://github.com/shantanujumde/meet-ai-app/releases/latest">Download</a> ·
  <a href="./CONTRIBUTING.md">Contributing</a> ·
  <a href="./LICENSE">License</a>
</p>

---

meet-ai records your microphone and the other people's audio on your own Mac,
turns both into a transcript on the device, and keeps everything as plain files
in a folder you own. Nothing is uploaded.

It is built with Tauri 2: a Rust core with a React window. On macOS 26 and later
it uses Apple's built-in speech engine, so there is no model to download.
Whisper is there as a fallback.

**Status:** v0.3.0, early. Apple silicon, macOS 26 or later. See
[`CHANGELOG.md`](./CHANGELOG.md) for what has shipped.

## Install

1. Download `meet-ai-X.Y.Z-macos-arm64.zip` from the
   [latest release](https://github.com/shantanujumde/meet-ai-app/releases/latest).
2. Unzip it and move `meet-ai.app` to `/Applications`.
3. The app is self-signed and not notarized by Apple, so macOS blocks the first
   launch. Clear that block once:

   ```sh
   xattr -dr com.apple.quarantine /Applications/meet-ai.app
   ```

To check the download, run `shasum -a 256 -c` against the `.sha256` file from
the same release.

## Run it locally

You need macOS 26 or later, Xcode Command Line Tools, Node 20.19 or later, and:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust; the version is pinned in rust-toolchain.toml
brew install just cmake
```

Then:

```sh
git clone https://github.com/shantanujumde/meet-ai-app.git
cd meet-ai-app
pnpm install
just check   # formatting, lint, types and every test. Green means healthy.
just dev     # run the app with hot reload
```

Recording audio needs a signed build, because macOS only keeps microphone and
system-audio permission for signed apps. One script makes a local signing
certificate, no password or admin rights needed:

```sh
./spikes/phase0a-tcc/make-identity.sh
just bundle-signed
```

[`CONTRIBUTING.md`](./CONTRIBUTING.md#4-code-signing--needed-before-any-audio-work)
has the details.

## Contributing

1. Fork the repo and make a branch from `main`.
2. Make your change. Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) first, and
   [`SPEC.md`](./SPEC.md) for the decisions that are already locked.
3. Run `just check` and make sure it passes.
4. Open a pull request against `main`. CI runs the same check, and PRs are
   squash-merged.

One rule matters more than the rest: nothing in this app may send meeting
audio, transcripts, titles or file names over the network. No telemetry, no
cloud AI calls.

Found a bug or have an idea? [Open an issue](https://github.com/shantanujumde/meet-ai-app/issues/new/choose).

## Security

Please report vulnerabilities privately. [`SECURITY.md`](./SECURITY.md) says how.

## Documents

| File | What it is for |
|---|---|
| [`SPEC.md`](./SPEC.md) | The build contract: locked decisions and the phase plan. It wins over everything else. |
| [`CONTRIBUTING.md`](./CONTRIBUTING.md) | Full setup, repo rules, and how to work on the code. |
| [`SETUP.md`](./SETUP.md) | Every pinned dependency version, and why. |
| [`RELEASING.md`](./RELEASING.md) | How to cut a release. |
| [`docs/problem.md`](./docs/problem.md) | What the product is for, and who it is for. |

## License

[Apache 2.0](./LICENSE). See also [`NOTICE`](./NOTICE).
