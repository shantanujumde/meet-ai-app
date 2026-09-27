# Phase 0a — macOS audio-capture permission spike

Answers the question in `SPEC.md` §5, Phase 0a:

> Inside a **signed** app bundle, can a helper acquire macOS audio-capture
> permission and actually receive non-silent system audio?

**Result: yes, all three parts.** Full write-up with measured numbers is in
[`FINDINGS.md`](../../FINDINGS.md) §8. Read that first — this file is just how to
run it again.

## What's here

| File | Role |
|---|---|
| `src/app/main.swift` | `Contents/MacOS/meet-ai` — stands in for the Tauri shell. Spawns the helper, plays a synthetic tone so there is guaranteed system audio |
| `src/probe/main.swift` | `Contents/MacOS/meet-tap-probe` — the helper. Core Audio process tap → `system.wav`, optional mic → `mic.wav`, measured stats → `probe-result.json` |
| `src/WavWriter.swift` | Incremental float32 WAV writer, header patched every 1 s (crash safety) |
| `Info-app.plist` | App `Info.plist`. Bundle ID `pro.saleschat.meetai` (⛔ locked, SPEC §8.1), `NSAudioCaptureUsageDescription`, `NSMicrophoneUsageDescription` |
| `Info-helper.plist` | Embedded in the helper via `-sectcreate __TEXT __info_plist`, as SPEC §5 specifies |
| `entitlements.plist` | `com.apple.security.device.audio-input` |
| `build.sh` | Compile → assemble bundle → sign with hardened runtime → verify |
| `make-identity.sh` | Creates a local self-signed code-signing identity (needs one `sudo` for the trust setting) |
| `run.sh` | Reset TCC, launch, then **re-measure the WAV with ffmpeg**, independently of what the probe claimed |

## Running it

```sh
./build.sh                         # ad-hoc signed
./run.sh --seconds 12 --reset      # system audio only
./run.sh --seconds 20 --mic        # also exercise the Microphone TCC service
```

For a stable TCC grant across rebuilds, use a real identity instead of ad-hoc:

```sh
./make-identity.sh
sudo security add-trusted-cert -d -r trustRoot -p codeSign \
    -k /Library/Keychains/System.keychain "$TMPDIR/meet-ai-signing/cert.pem"
SIGN_IDENTITY="meet-ai Local Signing" \
  SIGN_KEYCHAIN="$HOME/Library/Keychains/meet-ai-signing.keychain-db" ./build.sh
```

Artifacts land in `/tmp/meet-ai-phase0a-run` (override with `MEET_AI_SPIKE_OUT`).
Deliberately outside the repo — recordings must never be committable.

### Forcing a fresh permission prompt

Under ad-hoc signing TCC keys the grant to the executable **path**
(`identifier_type=Path`), and `tccutil reset AudioCapture pro.saleschat.meetai`
does not clear it. Copy the bundle somewhere new instead:

```sh
FRESH=/tmp/meet-ai-fresh-$(date +%s); mkdir -p "$FRESH"
cp -R build/meet-ai.app "$FRESH/"
open -a "$FRESH/meet-ai.app" --args --out /tmp/meet-ai-probe --seconds 20
```

### Watching TCC decide

```sh
/usr/bin/log show --last 5m --style compact --predicate 'subsystem == "com.apple.TCC"' \
  | grep -E "AUTHREQ_PROMPTING|AUTHREQ_ATTRIBUTION|Publishing <TCCDEvent"
```

`/usr/bin/log` spelled out in full — `log` is shadowed by a builtin in zsh.

## Why the measurement is shaped this way

An `OSStatus` of 0 is worthless here: `AudioHardwareCreateProcessTap` returns
`noErr` in ~3 ms whether or not the user has granted anything. So the probe
reports only things that come from the samples — frame count, RMS, peak, count of
bit-exact-zero samples, per-second RMS, and first/last buffer host timestamps —
and `run.sh` then re-measures the same file with `ffmpeg`, which has no idea what
the probe claimed.

The control case matters as much as the positive one: `./run.sh --no-tone` must
come back as **all zero bytes**. If it doesn't, the tap is picking up something
other than what we think it is.
