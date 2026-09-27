# Security Policy

## Supported versions

meet-ai is a personal-distribution app at this stage (SPEC.md §8.1 — public
release is a v2 target). Only the latest tagged release is supported.

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ |
| < 0.1   | ❌ |

## Reporting a vulnerability

Report privately through GitHub's
[security advisory form](https://github.com/shantanujumde/meet-ai-app/security/advisories/new).
Please do not open a public issue for a vulnerability.

Include what you did, what happened, and what you expected. A proof of concept
helps but is not required.

Expect an acknowledgement within 7 days. Because this is a single-maintainer
personal project, there is no committed fix timeline — the response will say
what the plan is.

## What is in scope

- Audio or transcript data leaving the machine when the user did not ask for
  it. The product's core claim is that audio never leaves the device, and
  there is no telemetry of any kind (SPEC.md §8.1).
- Bypassing the macOS TCC permission model, or causing the app to record
  without consent.
- Anything that lets a local process read another user's meeting files, or
  escalate through the app's entitlements.
- Code execution through a downloaded speech model, a `segments.json` file, or
  any other untrusted input the app parses.
- Weaknesses in the updater signature path (`tauri-plugin-updater`).

## What is out of scope

- The self-signed local signing identity used for personal builds. It is
  documented as self-signed and is not a trust claim; Developer ID signing and
  notarization are v2 items.
- The placeholder updater endpoint (`https://example.invalid/...`). The updater
  ships with `active: false` and reaches no network.
- Anything requiring physical access to an unlocked machine that is already
  running the app.
- Third-party model weights downloaded by `crates/modelfetch`. Those are
  checksum-pinned against their upstream publisher; report integrity issues to
  the publisher, and report checksum-verification bugs here.

## Security properties worth knowing

- **No telemetry, ever.** Not a setting — it is absent from the code.
- **Network use is narrow.** Only two paths touch the network: downloading a
  whisper model, and installing an Apple on-device locale. Transcription
  itself is fully offline.
- **Entitlements are minimal.** The app holds
  `com.apple.security.device.audio-input` and nothing else. Nested helper
  binaries hold no entitlements at all.
- **Cloud is opt-in and bring-your-own-key.** No keys are bundled.
