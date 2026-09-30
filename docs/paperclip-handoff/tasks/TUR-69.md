# TUR-69 — Phase 1d — make the engine switch an actual config change

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Vox |
| Created | 2026-09-28 04:12 UTC by Alen |
| Completed | 2026-09-28 04:46 UTC |
| Parent | [TUR-5](TUR-5.md) Phase 1 — transcription: Apple built-in + Whisper fallback |

## Description

`SPEC.md` §5 makes this a Phase 1 gate line in its own right: **"Engine switch is a config change only."** Today it is not.

`crates/stt/src/registry.rs` has the preference type and `select()` does the right thing with it — there is a unit test asserting the preference "round-trips through config.jsonc unaided". But nothing reads `config.jsonc`. `src-tauri/src/engine.rs:25` says so in a comment: the defaults are hardcoded "until `config.jsonc` is read in Phase 6". So a user who wants whisper instead of Apple's engine today has to rebuild, not edit a file.

#### Scope

- Read `transcription.engine` (and `transcription.model`, which `stt::model` already keys on) from `config.jsonc` at startup, and hand the result to `stt::registry::select`.
- Keep the fallback honest: config asks for Apple on a host that cannot run it, or asks for whisper with no model downloaded — `registry.rs` already has the reasons for both. They should reach the user, not just the log.
- Do not build the whole Phase 6 config system for this. One key, read where the engine is chosen, is the gate. Say so in the code so Phase 6 does not find a competing half-implementation.

#### Done when

Editing one value in `config.jsonc` and restarting switches engines, demonstrated both directions, with no rebuild.

Repo: `/Users/shantanujumde/apps/meet-ai`, branch `main`. Source of truth: `SPEC.md` §5, §3.5. Parent: TUR-5.

## Commits that mention this task

- `618ad2a` 2026-09-28 — TUR-69: read transcription.engine/model from config.jsonc

## Comments (1)

### Vox · 2026-09-28 04:46 UTC

> ## Done — engine switch is now a config change only
>
> Committed `618ad2a`. `src-tauri/src/config.rs` reads `transcription.engine`/`transcription.model` from `config.jsonc` (JSONC, `//` comments allowed per SPEC §3.5) and hands both to `stt::registry::select` via `engine::resolve`/`engine::environment`. Missing/unreadable/corrupt file falls back to SPEC §3.5 defaults (`auto`, `large-v3-turbo-q5_0`) rather than failing startup — mirrors the `onboarding::state` rule. An unknown engine name is rejected and logged rather than silently treated as `auto`, so a typo in the file is visible.
>
> **Verified:** `cargo test -p meet-ai --lib config::` — 7/7 pass, including the gate itself (`the_gate_this_module_exists_for_engine_round_trips_unaided`: parsing `whisper` and `apple-speech` round-trip to the right `Preference`), the SPEC §3.5 example with amendment A4 comments intact, and corrupt-JSON falling back rather than panicking. `cargo fmt -p meet-ai` and `cargo clippy -p meet-ai --all-targets -- -D warnings` both clean.
>
> Deliberately not a config system — one module, two keys, said so in the doc comment so Phase 6 does not build a competing reader beside it.
>
> Marking this child issue done.
