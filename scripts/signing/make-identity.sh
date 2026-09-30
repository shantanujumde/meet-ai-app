#!/usr/bin/env bash
# Ensure the local self-signed code-signing identity for the spike exists.
#
# SPEC §2.9 calls for "local self-signed identity + hardened runtime" rather than
# ad-hoc signing, because TCC keys an ad-hoc signature to the binary's cdhash:
# every rebuild produces a new hash and silently drops the grant. A self-signed
# leaf gives a stable designated requirement instead:
#
#   ad-hoc:    designated => cdhash H"8a42ac53…"              (changes every build)
#   identity:  designated => identifier "pro.saleschat.meetai" and
#                            certificate leaf = H"eafb73d2…"  (stable)
#
# THIS SCRIPT IS IDEMPOTENT ON PURPOSE. The leaf SHA-1 it prints is what TCC
# keys every grant to. Minting a fresh cert changes it, orphaning every existing
# grant and making a re-prompt ambiguous — is TCC correctly re-asking after a
# denial, or does it just not recognise the binary any more? A
# denied-then-re-granted gate cannot be certified against a moving identity
# (TUR-2, TUR-10), so re-running this must be a no-op. --rotate to override.
#
#   scripts/signing/make-identity.sh            # ensure it exists; no-op if it already does
#   scripts/signing/make-identity.sh --rotate   # mint a new cert (invalidates every TCC grant)
#   scripts/signing/make-identity.sh --print    # just report the current fingerprint
#
# No admin password is needed — measured, TUR-10. The `-d` (admin domain) trust
# flag in this ticket's original recipe was the only thing that required one.
set -euo pipefail

NAME="meet-ai Local Signing"
KEYCHAIN_SHORT="meet-ai-signing.keychain"
PASS="meetai"
MODE="ensure"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rotate) MODE="rotate"; shift ;;
    --print)  MODE="print";  shift ;;
    -*) echo "unknown flag: $1" >&2; exit 2 ;;
    *)  NAME="$1"; shift ;;
  esac
done

# `security` resolves a short keychain name against the *account's* home, not
# against $HOME. Under a sandboxed or redirected $HOME those two disagree and
# the cert lands somewhere the keychain isn't — which is how TUR-10's first run
# ended up reporting "0 valid identities" for an identity that was there all
# along. Ask directory services for the real home so the two always agree.
REAL_HOME="$(/usr/bin/dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
[[ -n "$REAL_HOME" && -d "$REAL_HOME" ]] || REAL_HOME="$HOME"

KEYCHAIN="$REAL_HOME/Library/Keychains/$KEYCHAIN_SHORT-db"
# NOT $TMPDIR. The first version kept the cert under $TMPDIR/meet-ai-signing;
# macOS purges /var/folders/.../T, and under Paperclip $TMPDIR is per-run, so
# the PEM vanished within the day while the keychain survived. The keychain is
# the source of truth now and this directory is only a cache of its public half.
DIR="$REAL_HOME/.meet-ai/signing"

fingerprint() {
  # SHA-1 over the DER cert — the exact value that appears in the bundle's
  # designated requirement as `certificate leaf = H"…"`.
  security find-certificate -c "$NAME" -p "$KEYCHAIN" 2>/dev/null \
    | openssl x509 -outform der 2>/dev/null \
    | shasum -a 1 | awk '{print $1}'
}

have_identity() {
  [[ -f "$KEYCHAIN" ]] \
    && security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null | grep -qF "$NAME"
}

report() {
  local fp; fp="$(fingerprint)"
  echo
  echo "identity:    $NAME"
  echo "keychain:    $KEYCHAIN"
  echo "leaf SHA-1:  ${fp:-(could not read)}"
  echo
  echo "Every TCC grant is keyed to that SHA-1. Assert the baseline before a gate run:"
  echo "  codesign -d -r- build/meet-ai.app 2>&1 | grep -qi '${fp:-????}' && echo baseline-ok"
  echo
  echo "use:  SIGN_IDENTITY=\"$NAME\" SIGN_KEYCHAIN=\"$KEYCHAIN\" spikes/phase0a-tcc/build.sh"
}

if [[ "$MODE" == "print" ]]; then
  have_identity || { echo "no identity '$NAME' in $KEYCHAIN — run scripts/signing/make-identity.sh" >&2; exit 1; }
  report
  exit 0
fi

if have_identity && [[ "$MODE" != "rotate" ]]; then
  echo "==> identity '$NAME' already exists — reusing it (that is the point)"
  mkdir -p "$DIR"
  # Re-cache the public cert so the trust step can be repeated without minting
  # a new one. The private key stays in the keychain; it is never written out.
  security find-certificate -c "$NAME" -p "$KEYCHAIN" > "$DIR/cert.pem" 2>/dev/null || true
  report
  exit 0
fi

if [[ "$MODE" == "rotate" ]] && have_identity; then
  echo "!! ROTATING. The current leaf is $(fingerprint)."
  echo "!! Every TCC grant keyed to it becomes an orphan record, and the app will"
  echo "!! prompt again as if never asked. Ctrl-C now unless that is what you want."
  echo "!! Afterwards, clear the stale records:"
  echo "!!   tccutil reset AudioCapture pro.saleschat.meetai"
  echo "!!   tccutil reset Microphone   pro.saleschat.meetai"
  sleep 5
fi

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

# The private key is in the keychain now; no reason to leave a second copy in a
# plain file that a later `cp -r` could scoop into the repo.
rm -f key.pem id.p12

# Put it on the user's keychain search list so codesign can find the key.
# `-s` REPLACES the list, so an empty $CURRENT would wipe the login keychain out
# of the search path. That happens whenever this runs with a redirected $HOME.
CURRENT=$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//')
if [[ -n "$CURRENT" ]]; then
  # shellcheck disable=SC2086
  security list-keychains -d user -s $CURRENT "$KEYCHAIN"
else
  echo "    !! user keychain search list came back empty — not touching it."
  echo "    !! build.sh passes --keychain explicitly, so signing still works."
fi

# The *user* trust domain is enough for codesign and needs no admin password —
# measured, TUR-10. `-d` (admin domain) is tried only as a fallback because it
# goes through SecurityAgent and blocks on a GUI password dialog, which a
# headless run cannot answer.
echo "==> marking the cert trusted for code signing (user trust domain, no admin)"
if security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" cert.pem 2>/dev/null; then
  echo "    trusted in the user domain (cert stored in $KEYCHAIN_SHORT)"
elif security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain cert.pem 2>/dev/null; then
  echo "    trusted in the system keychain"
else
  echo "    !! could not set trust automatically."
  echo "    !! Run this once, then re-run build.sh:"
  echo "    sudo security add-trusted-cert -d -r trustRoot -p codeSign \\"
  echo "        -k /Library/Keychains/System.keychain $DIR/cert.pem"
fi

report
