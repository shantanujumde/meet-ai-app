set shell := ["bash", "-uc"]

# Self-signed identity created by `spikes/phase0a-tcc/make-identity.sh`. That
# script is headless — no sudo, no Keychain Access, no admin password, because
# the trust setting goes in the *user* domain (measured, TUR-10) — so there is
# no manual GUI step in this repo's signing path. The default below is the name
# that script gives the cert; override both vars for a Developer ID build.
SIGN_IDENTITY := env_var_or_default("SIGN_IDENTITY", "meet-ai Local Signing")

# The cert lives in its own keychain so nothing in the login keychain is
# touched, which means codesign needs to be pointed at it. Empty = search the
# default keychains.
SIGN_KEYCHAIN := env_var_or_default("SIGN_KEYCHAIN", env_var("HOME") + "/Library/Keychains/meet-ai-signing.keychain-db")

# The one command. If this is green, the repo is healthy.
#
# `sidecar` is a dependency, not decoration. Two reasons:
#   1. swiftc is the only thing that compiles sidecar/meet-stt. Without it here
#      a broken main.swift passes `just check` and only fails at `just build`.
#      Survivable while the sidecar is a 60-line stub; not survivable in Phase 1
#      when it becomes the default engine on macOS 26.
#   2. crates/stt tests the sidecar across the process boundary rather than with
#      XCTest (see main.swift's header), so `cargo test` needs target/meet-stt
#      to already exist.
check: check-windows sidecar
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    pnpm biome check .
    pnpm typecheck
    pnpm vitest run

# Windows seam guard (SPEC §8.2) — platform-agnostic crates must keep compiling
# for Windows from day one, so a mac assumption cannot quietly leak out of
# crates/audio or crates/calendar.
#
# SETUP.md §6 lists four crates here. Two crates are deliberately left out, and
# both for the same reason: a dependency whose build script compiles C for the
# *target*, which needs an MSVC toolchain no Mac has. Including either would
# make this check permanently red for a reason that has nothing to do with our
# code, and a permanently-red guard is a guard nobody reads.
#
#   * `store` — rusqlite's `bundled` feature compiles sqlite3.c. Put it back the
#     day store stops pulling a target-compiled C dependency.
#   * `modelfetch` — reqwest's `rustls-tls` pulls `ring`, which compiles
#     curve25519.c and friends. `aws-lc-rs` is not an escape; it compiles C too.
#     This crate is ~200 lines of HTTP range-request and SHA-256 over
#     `std::path`, with no `#[cfg(target_os)]` anywhere, so it is the cheapest
#     possible thing to exempt. Put it back if rustls ever ships a usable
#     pure-Rust provider.
#
# `stt` IS covered, and staying that way is why `modelfetch` is a separate crate
# at all — the downloader was inside `stt` and took the whole speech-to-text
# crate out of this guard with it (TUR-13). whisper-rs is gated to macOS in
# stt's Cargo.toml, so nothing else in there compiles C for the target.
#
# `audio` is the crate this guard mainly exists for — it is the only one with an
# `#[cfg(target_os = "macos")]` module — and it was missing from the list, so the
# seam the comment above describes was not actually being checked. Its Apple
# framework dependencies are already gated in Cargo.toml, so it cross-checks
# clean; `cargo check` never links, so cpal's Windows backend needs no MSVC
# toolchain. `calendar` is here for the same reason, ahead of its Phase 5 deps.
check-windows:
    rustup target add x86_64-pc-windows-msvc
    cargo check --target x86_64-pc-windows-msvc -p audio -p calendar -p stt -p prompts -p detect

# Format and autofix everything that can be autofixed.
fmt:
    cargo fmt --all
    pnpm biome check --write .

# --- running ---------------------------------------------------------------

# The Swift speech helper (SPEC §2.6). swiftc ships with Command Line Tools;
# full Xcode is not required.
sidecar:
    mkdir -p target
    swiftc -O sidecar/meet-stt/main.swift -o target/meet-stt

dev: sidecar
    pnpm tauri dev

# Phase 0's capture CLI — no Tauri, no UI.
rec *ARGS:
    cargo run -p audio --bin meet-rec -- {{ARGS}}

stt FILE:
    ./target/meet-stt {{FILE}}

# Which speech engine can this Mac use right now, offline? This is what the
# registry runs to decide between Apple's engine and the whisper fallback.
stt-probe: sidecar
    ./target/meet-stt --probe

# Install Apple's on-device model for a locale. Needs the network, and it is
# the ONLY thing on the Apple path that does — transcription never touches it.
stt-install-locale LOCALE="en-US": sidecar
    ./target/meet-stt --install-locale --locale {{LOCALE}}

# --- whisper fallback models -----------------------------------------------

# List the pinned models and whether they are already downloaded.
models:
    cargo run -q -p modelfetch --bin meet-stt-model -- list

# Download one. Resumable and checksum-verified, so re-running after an
# interrupted attempt continues rather than starting over.
model ID="small.en-q5_1":
    cargo run -q --release -p modelfetch --bin meet-stt-model -- get {{ID}}

# The Phase 1 accuracy + silence gate against the whisper engine. Separate from
# `just check` because it needs a 190 MB model on disk; `just model` first.
check-whisper:
    cargo test -p stt --features whisper-model-tests -- --nocapture

build: sidecar
    pnpm tauri build

# --- signing ---------------------------------------------------------------

# macOS TCC will not reliably register an unsigned app, so audio permission
# never sticks. This is not optional, even for personal use.
#
# Nested code is signed first, bundle last, and `--deep` is NOT used to sign.
# codesign seals whatever it finds inside the bundle at the moment it signs the
# outer wrapper, so signing outside-in leaves a seal over binaries that were
# re-signed afterwards. `--deep` looks like it solves that but applies the same
# entitlements to every nested binary and is deprecated by Apple for signing;
# it is only used to *verify* below. The half-signed-bundle bug this avoids is
# a real one — see the comment in spikes/phase0a-tcc/build.sh.
sign:
    #!/usr/bin/env bash
    set -euo pipefail
    APP="target/release/bundle/macos/meet-ai.app"
    [[ -d "$APP" ]] || { echo "no bundle at $APP — run \`just build\` first" >&2; exit 1; }

    # Written as a function rather than an args array on purpose: under `set -u`
    # an empty array expansion aborts the script *between* the nested and outer
    # codesign calls, which silently leaves the app unsigned while the helpers
    # look fine. That exact bug shipped once already (build.sh, TUR-9).
    seal() {
      if [[ -n "{{SIGN_KEYCHAIN}}" && -f "{{SIGN_KEYCHAIN}}" ]]; then
        codesign --force --options runtime --timestamp=none \
          --entitlements src-tauri/entitlements.plist \
          --keychain "{{SIGN_KEYCHAIN}}" -s "{{SIGN_IDENTITY}}" "$1"
      else
        codesign --force --options runtime --timestamp=none \
          --entitlements src-tauri/entitlements.plist \
          -s "{{SIGN_IDENTITY}}" "$1"
      fi
    }

    # Every helper Tauri is supposed to embed must actually be in the bundle
    # before we seal anything. The loop below is a `find`, and a `find` that
    # matches nothing is indistinguishable from a `find` that matched and
    # signed everything: a sidecar that silently fails to copy gives a green
    # sign AND a green `--verify --deep`, because --deep cannot report a file
    # that is not there. That ships an app with no transcription helper and
    # nothing goes red. Found by Tess on TUR-2; the check is here rather than
    # with the `externalBin` change so it cannot be forgotten alongside it.
    #
    # plutil, not jq: it reads JSON, ships with macOS, and this recipe is
    # macOS-only already. A missing `externalBin` key exits 1 with no output,
    # which is the correct "nothing declared" answer for today.
    # `|| [[ -n "$want" ]]` is load-bearing: plutil emits no trailing newline,
    # so a plain `read` returns non-zero on the last entry and drops it. With a
    # single declared sidecar that silently skips the whole check.
    declared=0
    while IFS= read -r want || [[ -n "$want" ]]; do
      [[ -n "$want" ]] || continue
      declared=$((declared + 1))
      [[ -x "$APP/Contents/MacOS/$want" ]] || {
        echo "externalBin '$want' is declared in tauri.conf.json but is not in $APP/Contents/MacOS" >&2
        exit 1
      }
    done < <(plutil -extract bundle.externalBin json -o - src-tauri/tauri.conf.json 2>/dev/null \
               | tr -d '[]"' | tr ',' '\n' | sed -e 's:.*/::' -e '/^$/d')

    # Sidecars and helpers, inside-out, so the outer seal covers final bytes.
    # Counted and printed: "0 nested" must be a statement, not a silence.
    sealed=0
    while IFS= read -r -d '' nested; do
      echo "==> sealing nested: ${nested#"$APP/"}"
      seal "$nested"
      sealed=$((sealed + 1))
    done < <(find "$APP/Contents/MacOS" -type f -perm -u+x ! -name meet-ai -print0)
    echo "==> nested binaries: $sealed sealed, $declared declared in externalBin"

    echo "==> sealing bundle: $APP"
    seal "$APP"

# The ONLY valid environment for the Phase 0a TCC spike. `just dev` proves
# nothing: TCC keys on the signed bundle identity, and dev builds are unsigned
# at a different path.
#
# `--timestamp=none` above means a self-signed build carries no trusted
# timestamp; that is fine locally and MUST be dropped for a Developer ID
# release, which Apple will otherwise reject at notarisation.
bundle-signed: build sign
    codesign --verify --deep --strict --verbose=2 "target/release/bundle/macos/meet-ai.app"

# --- fixtures --------------------------------------------------------------

# Fixture generation (crates/audio/fixtures/). Needs ffmpeg and macOS `say`.
#
# Produces the three fixtures SPEC §6 names plus `room-tone-30s.wav`, which is
# the silence case that actually catches a too-permissive VAD. The WAVs are
# generated rather than committed — see generate.sh for why, and for the
# reference text the accuracy test measures word error rate against.
fixtures:
    bash crates/audio/fixtures/generate.sh
