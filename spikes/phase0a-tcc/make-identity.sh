#!/usr/bin/env bash
# Create a local self-signed code-signing identity for the spike.
#
# SPEC §2.9 calls for "local self-signed identity + hardened runtime" rather than
# ad-hoc signing, because TCC keys an ad-hoc signature to the binary's cdhash:
# every rebuild produces a new hash and silently drops the grant. A self-signed
# leaf gives a stable designated requirement instead.
#
# The cert lives in its own keychain so nothing in the login keychain is touched.
# Making codesign *accept* it still needs a trust setting, and that is the step
# that may ask for an admin password — it cannot be done silently by design.
set -euo pipefail

NAME="${1:-meet-ai Local Signing}"
DIR="${TMPDIR:-/tmp}/meet-ai-signing"
KEYCHAIN="$HOME/Library/Keychains/meet-ai-signing.keychain-db"
KEYCHAIN_SHORT="meet-ai-signing.keychain"
PASS="meetai"

mkdir -p "$DIR"
cd "$DIR"

echo "==> generating self-signed code-signing cert: $NAME"
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout key.pem -out cert.pem \
  -subj "/CN=$NAME" \
  -addext "basicConstraints=critical,CA:false" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" >/dev/null 2>&1

# Security.framework cannot read OpenSSL 3's default PKCS#12 encryption, so pin
# the legacy PBE algorithms. Without this the import fails with a misleading
# "MAC verification failed (wrong password?)".
openssl pkcs12 -export -out id.p12 -inkey key.pem -in cert.pem \
  -name "$NAME" -passout "pass:$PASS" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 >/dev/null 2>&1

if [[ -f "$KEYCHAIN" ]]; then
  security delete-keychain "$KEYCHAIN_SHORT" 2>/dev/null || true
fi
security create-keychain -p "$PASS" "$KEYCHAIN_SHORT"
security set-keychain-settings -lut 36000 "$KEYCHAIN_SHORT"
security unlock-keychain -p "$PASS" "$KEYCHAIN_SHORT"
security import id.p12 -k "$KEYCHAIN_SHORT" -P "$PASS" -T /usr/bin/codesign -A >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PASS" "$KEYCHAIN_SHORT" >/dev/null 2>&1

# Put it on the user's keychain search list so codesign can find the key.
CURRENT=$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//')
# shellcheck disable=SC2086
security list-keychains -d user -s $CURRENT "$KEYCHAIN"

echo "==> marking the cert trusted for code signing (may prompt for admin)"
if security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain cert.pem 2>/dev/null; then
  echo "    trusted in the system keychain"
elif security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" cert.pem 2>/dev/null; then
  echo "    trusted in $KEYCHAIN_SHORT"
else
  echo "    !! could not set trust automatically."
  echo "    !! Run this once, then re-run build.sh:"
  echo "    sudo security add-trusted-cert -d -r trustRoot -p codeSign \\"
  echo "        -k /Library/Keychains/System.keychain $DIR/cert.pem"
fi

echo
echo "==> codesigning identities now visible:"
security find-identity -v -p codesigning || true
echo
echo "use:  SIGN_IDENTITY=\"$NAME\" SIGN_KEYCHAIN=\"$KEYCHAIN\" ./build.sh"
