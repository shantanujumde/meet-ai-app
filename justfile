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
# SETUP.md §6 lists four crates here. `store` is the one omission: rusqlite's
# `bundled` feature compiles sqlite3.c for the *target*, which needs an MSVC
# toolchain no Mac has, so including it would make this check permanently red
# for a reason that has nothing to do with our code. Put it back the day store
# stops pulling a target-compiled C dependency.
#
# `stt` IS covered, because whisper-rs is gated to macOS in its Cargo.toml.
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

    # Sidecars and helpers, inside-out. No-op until tauri.conf.json gains an
    # `externalBin`; written now so embedding meet-stt does not silently produce
    # an unsigned helper inside a signed app.
    while IFS= read -r -d '' nested; do
      echo "==> sealing nested: ${nested#"$APP/"}"
      seal "$nested"
    done < <(find "$APP/Contents/MacOS" -type f -perm -u+x ! -name meet-ai -print0)

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

# One-time fixture generation (crates/audio/fixtures/). Needs ffmpeg.
fixtures:
    mkdir -p crates/audio/fixtures
    ffmpeg -y -f lavfi -i anullsrc=r=16000:cl=mono -t 30 -c:a pcm_s16le \
      crates/audio/fixtures/silence-30s.wav
