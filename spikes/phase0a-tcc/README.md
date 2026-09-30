# Phase 0a — macOS audio-capture permission spike

Answers the question in `SPEC.md` §5, Phase 0a:

> Inside a **signed** app bundle, can a helper acquire macOS audio-capture
> permission and actually receive non-silent system audio?

**Result: yes, all three parts.** Full write-up with measured numbers is in
[`docs/findings.md`](../../docs/findings.md) §8. Read that first — this file is just how to
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
| `verify-tur10.sh` | Grant creation, survival across a rebuild, and an explicit **Don't Allow**. Runs unattended with `AUTO_CLICK=1`; writes `/tmp/meet-ai-tur10/report.md` itself |
| `auto-click.sh` | Answers the TCC consent dialog via Accessibility. Refuses to click any window that is not our own prompt — see the safety notes in its header |

### Answering the consent prompt without a human

The dialog is drawn by `UserNotificationCenter` and is an ordinary Accessibility
window, so `verify-tur10.sh` can answer it itself (FINDINGS §10.7):

```sh
AUTO_CLICK=1 ./verify-tur10.sh     # ~2 min, no keyboard needed, plays a tone
```

This needs Accessibility for whatever runs it. Check with:

```sh
osascript -e 'tell application "System Events" to return count of every process'
```

A `-1743` error means it is not granted; the script says so and falls back to
asking you to click, rather than silently recording an unanswered prompt.

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

With a real identity — which is the only way `build.sh` will sign — this is all
you need, and it is repeatable indefinitely:

```sh
tccutil reset AudioCapture pro.saleschat.meetai
```

⛔ **Never copy the bundle to a new path to force a prompt.** Under ad-hoc
signing TCC keys the grant to the executable **path** (`identifier_type=Path`),
and each new path manufactures a **permanent** record: `tccutil reset` resolves
its argument through LaunchServices as a bundle ID and returns `-10814` for a
path, and the record outlives the directory it names. Several are already stuck
on the dev machine and cannot be removed (measured, FINDINGS §10.6). An earlier
revision of this section recommended exactly that; it is a one-way door. This
matches the ⛔ in SPEC §5 — sign with the identity and reset by bundle ID.

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
