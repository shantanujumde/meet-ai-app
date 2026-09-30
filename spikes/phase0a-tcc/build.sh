#!/usr/bin/env bash
# Build + assemble + sign the Phase 0a probe bundle.
#
# SPEC §5 requires a *signed release bundle*; `tauri dev` is not a valid TCC test
# environment. This script is the minimal stand-in for `just bundle-signed`:
# no Tauri, no React, same signing shape.
#
#   ./build.sh                     # signs with the local identity (see scripts/signing/make-identity.sh)
#   ALLOW_ADHOC=1 ./build.sh       # ad-hoc — only when you mean it, see below
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD="$HERE/build"
APP="$BUILD/meet-ai.app"
MACOS_DIR="$APP/Contents/MacOS"
DEPLOY_TARGET="arm64-apple-macos14.4"

rm -rf "$BUILD"
mkdir -p "$MACOS_DIR"

echo "==> compiling helper (meet-tap-probe)"
swiftc \
  -swift-version 5 \
  -target "$DEPLOY_TARGET" \
  -O \
  "$HERE/src/WavWriter.swift" "$HERE/src/probe/main.swift" \
  -framework CoreAudio -framework AudioToolbox -framework AVFoundation \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$HERE/Info-helper.plist" \
  -o "$MACOS_DIR/meet-tap-probe"

echo "==> compiling app (meet-ai)"
swiftc \
  -swift-version 5 \
  -target "$DEPLOY_TARGET" \
  -O \
  "$HERE/src/WavWriter.swift" "$HERE/src/app/main.swift" \
  -framework AppKit \
  -o "$MACOS_DIR/meet-ai"

cp "$HERE/Info-app.plist" "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# ---------------------------------------------------------------------------
# Signing. Inner binaries first, bundle last — codesign seals what it finds.
# Hardened runtime (--options runtime) is mandatory: SPEC §2.9 and §7 both say
# TCC will not reliably register the app without it, and the spike is supposed
# to test the real configuration, not a relaxed one.
# ---------------------------------------------------------------------------
# Default to the local identity and find its keychain ourselves, so nobody has
# to remember two env vars. scripts/signing/make-identity.sh is idempotent — running it twice
# does not rotate the cert.
IDENTITY="${SIGN_IDENTITY:-meet-ai Local Signing}"
if [[ -z "${SIGN_KEYCHAIN:-}" && "$IDENTITY" != "-" ]]; then
  REAL_HOME="$(/usr/bin/dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
  [[ -n "$REAL_HOME" && -d "$REAL_HOME" ]] || REAL_HOME="$HOME"
  CANDIDATE="$REAL_HOME/Library/Keychains/meet-ai-signing.keychain-db"
  [[ -f "$CANDIDATE" ]] && SIGN_KEYCHAIN="$CANDIDATE"
fi

# Ad-hoc signing is no longer a silent fallback. TCC cannot key an ad-hoc
# signature to anything stable, so it keys the grant to the executable's
# absolute PATH instead — and those path records are permanent: `tccutil reset`
# resolves its argument through LaunchServices as a bundle ID, so a path record
# is unreachable and errors with -10814 (measured, TUR-10). Every ad-hoc build
# in a fresh directory therefore leaves one more un-removable TCC record behind
# and makes the next grant test ambiguous. Two such orphans already exist.
if [[ "$IDENTITY" == "-" || -z "${SIGN_KEYCHAIN:-}" ]]; then
  if [[ "${ALLOW_ADHOC:-0}" != "1" ]]; then
    cat >&2 <<'EOF'
!! Refusing to ad-hoc sign.

   No code-signing identity was found, so this build would be signed ad-hoc.
   TCC would then key any grant to this bundle's absolute path, permanently:
   path-keyed records cannot be removed by `tccutil reset`, only by hand in
   System Settings. That poisons the permission baseline for everyone.

   Fix it once:   scripts/signing/make-identity.sh
   Or override:   ALLOW_ADHOC=1 ./build.sh
EOF
    exit 1
  fi
  IDENTITY="-"
  unset SIGN_KEYCHAIN
  echo "==> signing ad-hoc (ALLOW_ADHOC=1) — this will create a path-keyed TCC record"
else
  echo "==> signing with identity: $IDENTITY"
  echo "    keychain: $SIGN_KEYCHAIN"
fi

# Built as a function rather than an array of extra args: under `set -u`,
# expanding an empty array aborts the script, and it does so *between* the two
# codesign calls — which silently leaves the app bundle unsigned while the
# helper looks fine. That exact bug shipped in the first version of this file.
sign() {
  local target="$1"
  if [[ -n "${SIGN_KEYCHAIN:-}" ]]; then
    codesign --force --options runtime --timestamp=none \
      --entitlements "$HERE/entitlements.plist" \
      --keychain "$SIGN_KEYCHAIN" -s "$IDENTITY" "$target"
  else
    codesign --force --options runtime --timestamp=none \
      --entitlements "$HERE/entitlements.plist" \
      -s "$IDENTITY" "$target"
  fi
}

sign "$MACOS_DIR/meet-tap-probe"
sign "$APP"

echo "==> verifying"
codesign --verify --deep --strict --verbose=2 "$APP"
echo "--- app signature ---"
codesign -dv --entitlements - "$APP" 2>&1 || true
echo "--- helper signature ---"
codesign -dv --entitlements - "$MACOS_DIR/meet-tap-probe" 2>&1 || true

echo
echo "built: $APP"
