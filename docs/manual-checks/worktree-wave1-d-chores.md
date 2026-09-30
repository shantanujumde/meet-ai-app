# Manual checks: wave1-d-chores

Docs and repo chores (#14, #17). Nothing below was run by the agent; each needs a
Mac with the real setup, or network.

| What to run | Expected | Why skipped |
|---|---|---|
| `rustup target add x86_64-pc-windows-msvc && just check-windows` | Green, including the newly added `-p meeting-format` | Needs network for the rustup target; a cold cross-check is slow. CI and `just check` cover it. |
| `just kill-gate --help` | Prints the gate's usage | The recipe wraps `scripts/gates/tur97-kill/gate.sh`, which drives the real app, mic and speakers. Only `just --list` was checked. |
| `scripts/signing/make-identity.sh --print` | Prints the leaf SHA-1 of the existing identity (no change) | Touches the signing keychain. Only `bash -n` was run. |
| `scripts/signing/verify-tur10.sh` | Same report as before the move. It now finds `build.sh`, `auto-click.sh` and `src/` in `spikes/phase0a-tcc/` through a changed `HERE` line. | Needs a signing identity, a window and audio. Only `bash -n` was run. |

## Decisions taken without an answer

- Recipe name is `just kill-gate`, and it passes extra arguments through (`just kill-gate --end quit`).
- `README.md`, `RELEASING.md` and `spikes/phase0a-tcc/*` references to the two moved scripts were updated too, so no path points at a file that is gone.
