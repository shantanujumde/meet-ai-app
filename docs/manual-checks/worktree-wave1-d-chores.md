# Manual checks: wave1-d-chores

Docs and repo chores (#14, #17). Nothing below was run by the agent; each needs a
Mac with the real setup, or network.

| What to run | Expected | Why skipped |
|---|---|---|
| `rustup target add x86_64-pc-windows-msvc && just check-windows` | Green, including the newly added `-p meeting-format` | Needs network for the rustup target; a cold cross-check is slow. CI and `just check` cover it. |
| `just kill-gate --help` | Prints the gate's usage | The recipe wraps `scripts/gates/tur97-kill/gate.sh`, which drives the real app, mic and speakers. Only `just --list` was checked. |
| `scripts/signing/make-identity.sh --print` | Prints the leaf SHA-1 of the existing identity (no change) | Touches the signing keychain. Only `bash -n` was run. |
| `scripts/signing/verify-tur10.sh` | Same report as before the move. It now finds `build.sh`, `auto-click.sh` and `src/` in `spikes/phase0a-tcc/` through a changed `HERE` line. | Needs a signing identity, a window and audio. Only `bash -n` was run. |

## Results, 2026-10-01

- `just kill-gate --help` ✅ prints the gate's usage.
- `scripts/signing/make-identity.sh --print` ✅ prints leaf `eafb73d29b2f35ca25c2f9fd193869fd880a7e0d`, the same as the keychain; nothing changed.
- `AUTO_CLICK=1 scripts/signing/verify-tur10.sh` ✅ ran with nobody at the keyboard. The grant is keyed by bundle ID, it survives a rebuild with no prompt, and an explicit Don't Allow records silence while reporting success (`zero_sample_fraction` 1, `create_tap_osstatus` 0). No new path-keyed TCC rows. Side effects: it rebuilt `spikes/phase0a-tcc/build/meet-ai.app`, now signed with the current leaf (it was on the old `be3fb2c8…`, findings §11.1), and it leaves meet-ai's Microphone and System Audio grants reset.
- `just check-windows` not run.

## Decisions taken without an answer

- Recipe name is `just kill-gate`, and it passes extra arguments through (`just kill-gate --end quit`).
- `README.md`, `RELEASING.md` and `spikes/phase0a-tcc/*` references to the two moved scripts were updated too, so no path points at a file that is gone.
