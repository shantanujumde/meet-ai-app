# Manual checks: store crate internals (Phase 4, store part)

No manual checks needed. The change is a file split inside `crates/store`; everything runs headless.

Decisions taken without an answer from Shann:

- `store::Error` already uses `thiserror` and src-tauri matches `Error::Frontmatter`; left unchanged.
- No `pub` item was hidden. `store::frontmatter` is used by `crates/store/tests/adversarial.rs` and the search agent builds on the public API, so hiding anything risked breaking them.
- Dropped unused deps `chrono` and `jsonc-parser` (src-tauri keeps its own `jsonc-parser`). `cargo-udeps` is not installed; checked by grep.
