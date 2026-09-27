#!/usr/bin/env bash
# Build + assemble + sign the Phase 0a probe bundle.
#
# SPEC §5 requires a *signed release bundle*; `tauri dev` is not a valid TCC test
# environment. This script is the minimal stand-in for `just bundle-signed`:
# no Tauri, no React, same signing shape.
#
#   SIGN_IDENTITY="meet-ai Local Signing"  ./build.sh     # self-signed identity
#   ./build.sh                                            # falls back to ad-hoc (-)
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
IDENTITY="${SIGN_IDENTITY:--}"
KEYCHAIN_ARGS=()
if [[ -n "${SIGN_KEYCHAIN:-}" ]]; then KEYCHAIN_ARGS=(--keychain "$SIGN_KEYCHAIN"); fi

if [[ "$IDENTITY" == "-" ]]; then
  echo "==> signing ad-hoc (no code-signing identity configured)"
else
  echo "==> signing with identity: $IDENTITY"
fi

codesign --force --options runtime --timestamp=none \
  --entitlements "$HERE/entitlements.plist" \
  "${KEYCHAIN_ARGS[@]}" -s "$IDENTITY" "$MACOS_DIR/meet-tap-probe"

codesign --force --options runtime --timestamp=none \
  --entitlements "$HERE/entitlements.plist" \
  "${KEYCHAIN_ARGS[@]}" -s "$IDENTITY" "$APP"

echo "==> verifying"
codesign --verify --deep --strict --verbose=2 "$APP"
echo "--- app signature ---"
codesign -dv --entitlements - "$APP" 2>&1 || true
echo "--- helper signature ---"
codesign -dv --entitlements - "$MACOS_DIR/meet-tap-probe" 2>&1 || true

echo
echo "built: $APP"
