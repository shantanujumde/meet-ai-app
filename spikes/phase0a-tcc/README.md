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
| `make-identity.sh` | Ensures the local self-signed code-signing identity exists. **Idempotent** — re-running reuses it. **No `sudo`, no admin password** (TUR-10) |
| `run.sh` | Reset TCC, launch, then **re-measure the WAV with ffmpeg**, independently of what the probe claimed |
| `verify-tur10.sh` | The two checks that need a human: grant creation, and an explicit **Don't Allow**. Two clicks; writes `/tmp/meet-ai-tur10/report.md` itself |

## Running it

One-time, about seven seconds, asks for nothing:

```sh
./make-identity.sh                 # idempotent; re-running reuses the cert
```

Then:

```sh
./build.sh                         # signs with that identity, no env vars needed
./run.sh --seconds 12 --reset      # system audio only
./run.sh --seconds 20 --mic        # also exercise the Microphone TCC service
```

`tccutil reset AudioCapture pro.saleschat.meetai` works on a bundle built this
way, because the grant is keyed to the bundle ID rather than the executable
path — FINDINGS §10.4.

⛔ **Don't ad-hoc sign.** `build.sh` exits 1 rather than fall back to
`codesign -s -`, because an ad-hoc build makes TCC key its grant to the
executable's absolute path, and path-keyed records are permanent: `tccutil
reset` can't reach them and deleting the directory doesn't clear them
(FINDINGS §10.6). `ALLOW_ADHOC=1 ./build.sh` if you really mean it.

Likewise, don't re-mint the identity. The leaf SHA-1 that `./make-identity.sh
--print` reports is what every grant is keyed to; replacing it voids them all.
`--rotate` exists for when that is genuinely what you want.

Artifacts land in `/tmp/meet-ai-phase0a-run` (override with `MEET_AI_SPIKE_OUT`).
Deliberately outside the repo — recordings must never be committable.

### Forcing a fresh permission prompt

With a real identity, `tccutil reset AudioCapture pro.saleschat.meetai` is all
you need. Under **ad-hoc** signing TCC keys the grant to the executable **path**
(`identifier_type=Path`) and that reset is a silent no-op — copy the bundle
somewhere new instead:

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

TUR-10 made that stance load-bearing rather than merely careful: on a **denied**
capture *every* call in the chain returns `noErr`, the IO callbacks fire at the
normal rate, and every sample is a bit-exact zero (FINDINGS §10.1). Had the probe
trusted return codes it would have reported a clean pass on a recording that
contains nothing.

The control case matters as much as the positive one: `./run.sh --no-tone` must
come back as **all zero bytes**. If it doesn't, the tap is picking up something
other than what we think it is.
