set shell := ["bash", "-uc"]

# Self-signed identity created in Keychain Access. See Readme.md § Setup.
SIGN_IDENTITY := env_var_or_default("SIGN_IDENTITY", "meet-ai-dev")

# The one command. If this is green, the repo is healthy.
check: check-windows
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
check-windows:
    rustup target add x86_64-pc-windows-msvc
    cargo check --target x86_64-pc-windows-msvc -p stt -p prompts -p detect

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
sign:
    codesign --force --deep --options runtime \
      --entitlements src-tauri/entitlements.plist \
      -s "{{SIGN_IDENTITY}}" "target/release/bundle/macos/meet-ai.app"

# The ONLY valid environment for the Phase 0a TCC spike. `just dev` proves
# nothing: TCC keys on the signed bundle identity, and dev builds are unsigned
# at a different path.
bundle-signed: build sign
    codesign --verify --verbose=2 "target/release/bundle/macos/meet-ai.app"

# --- fixtures --------------------------------------------------------------

# One-time fixture generation (crates/audio/fixtures/). Needs ffmpeg.
fixtures:
    mkdir -p crates/audio/fixtures
    ffmpeg -y -f lavfi -i anullsrc=r=16000:cl=mono -t 30 -c:a pcm_s16le \
      crates/audio/fixtures/silence-30s.wav
