set shell := ["bash", "-uc"]
# Windows (TUR-36): PowerShell ships with every Windows, while which `bash` is
# first on PATH there (Git Bash, WSL, none) is not something to rely on. The
# recipes that run on Windows are plain commands either shell runs the same.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Self-signed identity created by `scripts/signing/make-identity.sh`. That
# script is headless — no sudo, no Keychain Access, no admin password, because
# the trust setting goes in the *user* domain (measured, TUR-10) — so there is
# no manual GUI step in this repo's signing path. The default below is the name
# that script gives the cert; override both vars for a Developer ID build.
SIGN_IDENTITY := env_var_or_default("SIGN_IDENTITY", "meet-ai Local Signing")

# The cert lives in its own keychain so nothing in the login keychain is
# touched, which means codesign needs to be pointed at it. Empty = search the
# default keychains.
SIGN_KEYCHAIN := env_var_or_default("SIGN_KEYCHAIN", env_var("HOME") + "/Library/Keychains/meet-ai-signing.keychain-db")

# Full Xcode, used by `icon-car` and nothing else. Pointed at per command via
# DEVELOPER_DIR rather than `xcode-select -s`, so swiftc, clang and cargo in
# every other recipe keep using Command Line Tools (SPEC A10).
XCODE_DEVELOPER_DIR := env_var_or_default("XCODE_DEVELOPER_DIR", "/Applications/Xcode.app/Contents/Developer")

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
#
# Off macOS (TUR-36) there is no swiftc and no Apple speech engine, so
# `sidecar` is a no-op recipe there (see it below) and this same recipe runs on
# Windows and Linux too. On Windows, `check-windows` is the native workspace
# build, test and clippy, the commands the `rust (windows)` CI job runs.
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
# SETUP.md §6 lists four crates here. Three crates are deliberately left out, and
# all for the same reason: a dependency whose build script compiles C for the
# *target*, which needs an MSVC toolchain no Mac has. Including any of them would
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
#   * `stt` — whisper-rs-sys compiles whisper.cpp with cmake for the target,
#     on every OS since TUR-52. The native `rust (windows)` CI job builds and
#     tests it instead.
#
# `stt` was covered until TUR-52 made whisper-rs a dependency on every OS
# (`modelfetch` was split out of it in TUR-13 to keep it covered). Its seam is
# now checked natively by `rust (windows)` and `rust (linux)` in CI.
#
# `audio` is the crate this guard mainly exists for — it is the only one with an
# `#[cfg(target_os = "macos")]` module — and it was missing from the list, so the
# seam the comment above describes was not actually being checked. Its Apple
# framework dependencies are already gated in Cargo.toml, so it cross-checks
# clean; `cargo check` never links, so cpal's Windows backend needs no MSVC
# toolchain. `calendar` is here for the same reason, ahead of its Phase 5 deps.
#
# Since TUR-42 every crate keeps its OS code in `src/platform/` (quality rule
# R10; `store` since TUR-89, `agent`'s tests since TUR-54), and `agent` joined
# the list: it has no C dependency at all. The second
# `cargo check` builds `audio` with `stub-audio`, the no-device sources CI on
# Windows and Linux runs tests with. The target is only added when missing.
#
# On a Windows machine (TUR-36) the recipe below is the real thing instead: a
# native build of the whole workspace, `src-tauri`, `store` and `modelfetch`
# included, since MSVC is there to compile their C. It is exactly what the
# `rust (windows)` CI job runs. The cross-check above stays for macOS and
# Linux, where it is the cheap early warning.
[windows]
check-windows:
    cargo build --workspace --all-targets
    cargo test --workspace --no-fail-fast
    cargo test -p audio --features audio/stub-audio
    cargo clippy --workspace --all-targets -- -D warnings

[unix]
check-windows:
    rustup target list --installed | grep -qx x86_64-pc-windows-msvc || rustup target add x86_64-pc-windows-msvc
    cargo check --target x86_64-pc-windows-msvc -p audio -p calendar -p prompts -p detect -p meeting-format -p agent
    cargo check --target x86_64-pc-windows-msvc -p audio --features audio/stub-audio

# The TUR-97 gate: does a recording survive the app dying mid-meeting? Drives the
# real installed app, so it needs a mic, speakers and a signed build; it is not
# part of `just check`. Options and what each check proves: scripts/gates/tur97-kill/README.md.
#
#   just kill-gate                          # 60 s, kill -9
#   just kill-gate --end quit               # Quit while recording
kill-gate *ARGS:
    bash scripts/gates/tur97-kill/gate.sh {{ARGS}}

# Format and autofix everything that can be autofixed.
fmt:
    cargo fmt --all
    pnpm biome check --write .

# --- running ---------------------------------------------------------------

# The Swift speech helper (SPEC §2.6). swiftc ships with Command Line Tools;
# full Xcode is not required.
#
# The second copy is for the bundle. tauri.conf.json's `externalBin` names
# `../target/meet-stt`, and Tauri looks for that path with the target triple
# appended, then ships it as `Contents/MacOS/meet-stt`, which is where
# `AppleEngine::discover` looks. Without it the app has no Apple engine (TUR-5)
# and says "the meet-stt sidecar was not found in the app bundle". tauri-build
# checks the file exists at compile time, so a bare `cargo build -p meet-ai`
# needs `just sidecar` first; every recipe here that builds the app runs it.
#
# Off macOS there is nothing to build (TUR-36): no swiftc, no SpeechAnalyzer,
# and `tauri.windows.conf.json` / `tauri.linux.conf.json` drop `externalBin`,
# so tauri-build does not look for the file. The no-op keeps `check`,
# `bindings` and the rest working there unchanged.
[linux]
[windows]
sidecar:
    @echo "no meet-stt sidecar off macOS"

[macos]
sidecar:
    mkdir -p target
    swiftc -O sidecar/meet-stt/main.swift -o target/meet-stt
    cp target/meet-stt "target/meet-stt-$(rustc -vV | sed -n 's/^host: //p')"

# Regenerate src/ipc/bindings.ts from the Rust command list. Headless: it runs a
# test, never the app. CI runs this and fails if the committed file differs.
bindings: sidecar
    cargo test -p meet-ai --lib export_bindings

dev: sidecar
    pnpm tauri dev

# A sample meetings folder, so the app shell's populated states can be looked at
# without recording a real meeting first. Writes into target/, never ~/Meetings.
ui-fixtures:
    bash src-tauri/fixtures/ui-fixtures.sh

# Run the app against those fixtures. This is the command to use when working on
# the meeting list, the review view or the notes pane — `just dev` points at the
# real ~/Meetings, which on a fresh machine is empty.
dev-ui: sidecar ui-fixtures
    MEET_AI_MEETINGS_ROOT="$PWD/target/ui-fixtures/Meetings" pnpm tauri dev

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

# --- live transcription ----------------------------------------------------

# Play a recorded meeting through the live session API and print what the
# Phase 2 pane would be told, one JSON object per line on stdout.
#
# This is how the live pane gets built before `meet-rec` has a live tap: no
# microphone, no model, no permissions. Both tracks run as two concurrent
# sessions sharing one `seq` counter, which is the shape the real recorder
# will use. stdout is NDJSON and nothing else, so it pipes; progress and the
# transcript that actually reached disk go to stderr.
#
# Not a `fixtures` dependency: the generator would re-run on every demo, and
# the example already says `just fixtures` when the WAVs are not there.
#
#   just live-replay                          # the 60 s fixture, in real time
#   just live-replay ARGS="--speed 8"         # same, eight times faster
#   just live-replay ARGS="--meeting ~/Meetings/2026-09-27-standup"
live-replay ARGS="":
    cargo run -q -p stt --example live_replay -- {{ARGS}}

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

# Assets.car is committed, so on a normal checkout the first line never fires.
# When it does, something deleted the file, and that is a real error, not a
# "skip the new icon" case: building on without it would ship the legacy .icns
# alone with nothing going red. Tauri fails on it too, but only after the
# frontend build and cargo, and without saying where the file comes from.
# Build the release app bundle. Fails fast if src-tauri/icons/Assets.car is missing.
build: sidecar
    @[[ -f src-tauri/icons/Assets.car ]] || { echo "src-tauri/icons/Assets.car is missing. It is committed, so restore it from git; if the .icon changed, regenerate it with \`just icon-car\` (needs Xcode 26+)." >&2; exit 1; }
    pnpm tauri build

# --- app icon ----------------------------------------------------------------

# The Icon Composer icon, compiled (TUR-85). This is the ONLY recipe that needs
# full Xcode: `build` just copies the committed Assets.car, so routine builds
# stay on Command Line Tools (SPEC A10).
#
# No post-bundle copy and no extra re-sign. tauri-bundler 2.9 (what
# @tauri-apps/cli 2.11.4 ships) treats a `.car` in `bundle.icon` as a compiled
# asset catalog: it copies it to Contents/Resources/Assets.car while it lays
# the bundle out, keeps the .icns as CFBundleIconFile beside it, and merges
# src-tauri/Info.plist (which carries CFBundleIconName) over the result. All of
# that happens before `just sign`, whose outer seal then covers Assets.car like
# any other resource. The plan on TUR-35 assumed Tauri had no asset-catalog
# step; this version does. Tauri could also run actool itself if handed the
# `.icon` directly, but then every `just build` would need Xcode — the thing
# committing the .car exists to avoid.
#
# The name passed to --app-icon is the .icon's filename stem, and it has to
# equal CFBundleIconName in src-tauri/Info.plist or macOS finds no icon of that
# name in the catalog and silently uses the .icns. So that is checked against
# three things: Info.plist, actool's own partial Info.plist, and what is really
# in the compiled catalog (assetutil). The partial plist and the .icns actool
# also writes stay in a temp dir — render.sh owns the .icns, and its TUR-22
# small-end decision is not something actool should overwrite.
#
# Compile meet-ai.icon into src-tauri/icons/Assets.car (needs Xcode 26+); commit the result.
icon-car:
    #!/usr/bin/env bash
    set -euo pipefail
    ICON="design-system/meet-ai/brand/meet-ai.icon"
    OUT="src-tauri/icons/Assets.car"
    NAME="$(basename "$ICON" .icon)"
    export DEVELOPER_DIR="{{XCODE_DEVELOPER_DIR}}"
    XCODEBUILD="$DEVELOPER_DIR/usr/bin/xcodebuild"

    [[ -d "$ICON" ]] || { echo "no Icon Composer source at $ICON (a .icon is a directory)" >&2; exit 1; }
    [[ -x "$XCODEBUILD" ]] || {
      echo "full Xcode 26+ was not found at $DEVELOPER_DIR." >&2
      echo "Install it, or point XCODE_DEVELOPER_DIR at the one you have. Command Line Tools have no actool." >&2
      exit 1
    }

    # xcrun refuses every Xcode tool until the licence is accepted, and says
    # so on stderr; pass that through with the exact fix. The full xcodebuild
    # path is spelled out because `sudo` drops DEVELOPER_DIR, and a bare
    # `sudo xcodebuild` would act on whatever xcode-select points at.
    if ! probe="$(xcrun --find actool 2>&1)"; then
      echo "actool is not usable from $DEVELOPER_DIR:" >&2
      echo "  $probe" >&2
      if [[ "$probe" == *licen* ]]; then
        echo "fix: sudo \"$XCODEBUILD\" -license accept" >&2
      fi
      exit 1
    fi

    # actool before 26 does not know the .icon format (Tauri refuses < 26 for
    # the same reason). Unparseable is not fatal: actool itself will reject the
    # input if it is too old, and loudly. `|| true` is what makes that so —
    # under pipefail a failing `actool --version` fails the assignment, and
    # `set -e` would end the recipe here with no message.
    version="$(xcrun actool --version 2>/dev/null \
                 | sed -n '/short-bundle-version/{n;s:.*<string>\([0-9][0-9]*\).*:\1:p;}' || true)"
    if [[ -n "$version" && "$version" -lt 26 ]]; then
      echo "actool $version is too old for a .icon; Xcode 26+ is required" >&2
      exit 1
    fi

    want="$(plutil -extract CFBundleIconName raw -o - src-tauri/Info.plist 2>/dev/null || true)"
    [[ "$want" == "$NAME" ]] || {
      echo "src-tauri/Info.plist has CFBundleIconName '${want:-<missing>}', but the icon is $NAME.icon" >&2
      echo "they must match or macOS never finds the icon in Assets.car" >&2
      exit 1
    }

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    echo "==> actool $(xcrun --find actool) ($NAME.icon, macOS 26.0)"
    if ! xcrun actool "$ICON" --compile "$tmp" \
         --output-format human-readable-text --notices --warnings --errors \
         --output-partial-info-plist "$tmp/partial.plist" \
         --app-icon "$NAME" --include-all-app-icons \
         --enable-on-demand-resources NO \
         --development-region en \
         --target-device mac \
         --minimum-deployment-target 26.0 \
         --platform macosx > "$tmp/actool.log" 2>&1; then
      cat "$tmp/actool.log" >&2
      echo "actool failed. On a freshly installed Xcode it usually wants its first-launch" >&2
      echo "packages: sudo \"$XCODEBUILD\" -runFirstLaunch" >&2
      exit 1
    fi
    # Notices and warnings are printed even on success: an Icon Composer file
    # can compile with a layer quietly dropped, and this is the only place
    # that would say so.
    cat "$tmp/actool.log"

    [[ -s "$tmp/Assets.car" ]] || { echo "actool exited 0 but wrote no Assets.car" >&2; exit 1; }
    got="$(plutil -extract CFBundleIconName raw -o - "$tmp/partial.plist" 2>/dev/null || true)"
    [[ "$got" == "$NAME" ]] || {
      echo "actool's partial Info.plist names the icon '${got:-<missing>}', expected $NAME" >&2
      exit 1
    }
    "{{just_executable()}}" _car-has-icon "$tmp/Assets.car" "$NAME"

    cp "$tmp/Assets.car" "$OUT"
    echo "==> wrote $OUT ($(wc -c < "$OUT" | tr -d ' ') bytes); commit it"

# Does this Assets.car hold an app icon with this name? Shared by `icon-car`
# (on actool's output) and `sign` (on the bundle's copy), so the two cannot
# drift apart.
#
# assetutil ships with macOS itself, not with Xcode, so this runs on a Command
# Line Tools machine. Its output is a pretty-printed JSON array, one object per
# asset; the app icon is the object whose AssetType is "Icon Image". awk rather
# than jq or python for the same reason `sign` uses plutil: nothing to install.
# A colour or layer asset can share the icon's name, which is why the type is
# matched too, not the name alone. Any parse miss comes out as "not found", so
# a change in assetutil's format goes red rather than passing.
[private]
_car-has-icon CAR NAME:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ -s "{{CAR}}" ]] || { echo "no Assets.car at {{CAR}}" >&2; exit 1; }
    if ! assetutil --info "{{CAR}}" | awk -v n="{{NAME}}" '
           /^  \{/ { t = 0; name = "" }
           /^    "AssetType" : "Icon Image"/ { t = 1 }
           /^    "Name" : "/ { name = $0; sub(/^    "Name" : "/, "", name); sub(/",?$/, "", name) }
           /^  \}/ { if (t && name == n) found = 1 }
           END { exit !found }'; then
      echo "{{CAR}} has no app icon named '{{NAME}}' (assetutil --info)" >&2
      exit 1
    fi

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

    # `security` (and the plain $HOME-based default above) resolves against the
    # *account's* home, not against $HOME — TUR-10, and it bit again on TUR-128:
    # under any redirected-$HOME session (a Paperclip agent run, sandbox-exec,
    # some CI runners) {{SIGN_KEYCHAIN}} points at a path that was never
    # created, `-f` is false, and this fell through to a bare `-s` with no
    # `--keychain` — which the default search list cannot resolve either, so
    # `codesign` fails outright ("no identity found"). make-identity.sh and
    # spikes/phase0a-tcc/build.sh already dodge this with a dscl lookup; do the
    # same here so `just sign` finds the one persistent identity regardless of
    # what $HOME happens to be this run.
    REAL_HOME="$(/usr/bin/dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
    [[ -n "$REAL_HOME" && -d "$REAL_HOME" ]] || REAL_HOME="$HOME"
    EFFECTIVE_KEYCHAIN="{{SIGN_KEYCHAIN}}"
    if [[ ! -f "$EFFECTIVE_KEYCHAIN" ]]; then
      CANDIDATE="$REAL_HOME/Library/Keychains/meet-ai-signing.keychain-db"
      [[ -f "$CANDIDATE" ]] && EFFECTIVE_KEYCHAIN="$CANDIDATE"
    fi

    # Written as functions rather than an args array on purpose: under `set -u`
    # an empty array expansion aborts the script *between* the nested and outer
    # codesign calls, which silently leaves the app unsigned while the helpers
    # look fine. That exact bug shipped once already (build.sh, TUR-9).
    # (`"$@"` below is safe where `"${arr[@]}"` is not — bash 3.2 special-cases
    # it, and /bin/bash on macOS is still 3.2.)
    #
    # $1 = path to sign, $2.. = extra flags.
    _codesign() {
      local path="$1"; shift
      if [[ -n "$EFFECTIVE_KEYCHAIN" && -f "$EFFECTIVE_KEYCHAIN" ]]; then
        codesign --force --options runtime --timestamp=none \
          --keychain "$EFFECTIVE_KEYCHAIN" -s "{{SIGN_IDENTITY}}" "$@" "$path"
      else
        codesign --force --options runtime --timestamp=none \
          -s "{{SIGN_IDENTITY}}" "$@" "$path"
      fi
    }

    # The app gets the mic entitlement, because the app is what records.
    seal() { _codesign "$1" --entitlements src-tauri/entitlements.plist; }

    # Nested helpers get none. entitlements.plist is a single key,
    # com.apple.security.device.audio-input, and meet-stt reads finished WAVs
    # off disk — it has no business holding mic access. Note this is the *same*
    # defect the header attributes to `--deep`: one entitlement set stamped
    # over every nested binary. Avoiding `--deep` but passing the app's plist
    # to each nested codesign call reproduces it exactly, which is what this
    # split prevents. Raised by Rune on TUR-2.
    seal_nested() { _codesign "$1"; }

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

    # Same failure shape for the Icon Composer icon (TUR-85). A missing
    # Assets.car, or a CFBundleIconName that names nothing inside it, is not an
    # error to macOS: it falls back to the legacy icon.icns, the app still
    # launches with an icon, and sign + `--verify --deep` are both green. So it
    # is checked here, before the seal, and only when tauri.conf.json actually
    # declares a .car in bundle.icon — the same "declared means required" rule
    # as externalBin above. Tauri always names the copy Contents/Resources/
    # Assets.car, whatever the source file is called.
    # Captured, not piped into `grep -q`: under pipefail, grep exiting early
    # can SIGPIPE plutil, fail the pipeline, and skip this check with no output.
    declared_icons="$(plutil -extract bundle.icon json -o - src-tauri/tauri.conf.json 2>/dev/null || true)"
    if [[ "$declared_icons" == *'.car"'* ]]; then
      icon_name="$(plutil -extract CFBundleIconName raw -o - "$APP/Contents/Info.plist" 2>/dev/null || true)"
      [[ -n "$icon_name" ]] || {
        echo "bundle.icon declares an Assets.car but $APP/Contents/Info.plist has no CFBundleIconName" >&2
        exit 1
      }
      "{{just_executable()}}" _car-has-icon "$APP/Contents/Resources/Assets.car" "$icon_name"
      echo "==> app icon: Assets.car holds '$icon_name' (CFBundleIconName)"
    fi

    # Sidecars and helpers, inside-out, so the outer seal covers final bytes.
    # Counted and printed: "0 nested" must be a statement, not a silence.
    sealed=0
    while IFS= read -r -d '' nested; do
      echo "==> sealing nested (no entitlements): ${nested#"$APP/"}"
      seal_nested "$nested"
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
