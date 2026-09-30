# TUR-81 — Engine not detected

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Vox |
| Created | 2026-09-28 06:00 UTC by You (board) |
| Completed | 2026-09-28 06:16 UTC |

## Description

![](/api/assets/68632cb4-a2b0-44ae-9050-66c03c78835d/content)



if speech engine is not ready tell user how to make it ready, if incompatable tell why incompatable etc

## Comments (2)

### Vox · 2026-09-28 06:16 UTC

> **Root cause:** the screenshot's "no speech engine is ready: Apple's is unavailable and no whisper model is downloaded" message never explained *why* Apple's engine was unavailable. That mattered because the code already computed the precise reason (sidecar missing from this build, this Mac's OS is below macOS 26, unsupported hardware/language, or the on-device locale model not installed) — but only used it when a user force-selected Apple's engine via config. The Auto path, which is what almost every user hits by default, discarded that detail and showed the same vague sentence for a genuinely incompatible Mac as for one that just hadn't downloaded a model yet.
>
> **Fix** (`crates/stt/src/registry.rs`): extracted the reason-diagnosis logic into `apple_unavailable_detail()` and reused it in the Auto path, for both outcomes:
> - Falls back to whisper successfully → reason now reads e.g. "Apple's speech engine is unavailable (the meet-stt sidecar was not found in the app bundle), so the whisper fallback is used".
> - Nothing usable at all → error now reads e.g. "no speech engine is ready: Apple's speech engine cannot be used (this Mac is below macOS 26), and no whisper model is downloaded yet — download one below to continue", separating the incompatibility from the actionable next step.
>
> No frontend changes were needed: `src/ipc/errors.ts` and the Settings/Onboarding screens already render the Rust message verbatim next to the "Download" button (that's the monospace line in the screenshot), so the improved wording flows straight through.
>
> **Verification:** added 2 unit tests pinning this behavior; all 6 `registry::` tests pass, `cargo clippy -p stt` and `cargo fmt --check` are clean. I could not do a live on-device check (no access to a pre-macOS-26 Mac or a packaged build missing the sidecar in this environment) — this is verified at the message-logic level via tests, not against real hardware. Committed as `bf9480d`.
>
> Note: `crates/audio/src/permission_check.rs` currently fails a full `cargo check -p meet-ai` build (unrelated cpal API mismatch) — that's pre-existing/another agent's in-progress work in this shared checkout, not something my change touches or caused; `cargo check -p stt` and `-p meet-ai`'s dependency on stt build fine on their own.

### Alen · 2026-09-28 06:06 UTC

> Routed to you (Vox) from TUR-84 — engine readiness lives in meet-stt, which is your area from Phase 1. Two parts: (1) the engine is reported as not detected when the user expects it to work, and (2) when it genuinely is not ready, the app should say how to make it ready, and when it is incompatible, say why. The wording surfaces in the app shell, so coordinate with Nia if the UI needs a change beyond the message text.
