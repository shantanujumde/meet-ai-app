//! Every Tauri event name, in one place (quality rule R2).
//!
//! The webview listens for these by name, so a rename on one side alone would
//! go quiet without an error. `bindings.rs` exports the same constants into
//! `src/ipc/bindings.ts`, and `client.ts` uses them from there.

/// Every recorder transition. A recording Rust ended on its own (TUR-97), or a
/// ⌘⇧R or menu-bar press it refused (TUR-127), arrives on it too, as an idle
/// status with `error` set — there is no separate error event.
pub const RECORDING_STATE_EVENT: &str = "recording://state";

/// Progress for one model download.
pub const MODEL_PROGRESS_EVENT: &str = "model://progress";

/// A change in the macOS permission readout.
pub const PERMISSION_STATUS_EVENT: &str = "permission://status";

/// Every live `LiveUpdate`, serialized as `stt` defines it: `kind` is
/// `volatile`/`final`/`dropped`, `speaker` is `you`/`others`.
pub const TRANSCRIPT_UPDATE_EVENT: &str = "transcript://update";

/// Every change of the live transcript's `Status`.
pub const TRANSCRIPT_STATUS_EVENT: &str = "transcript://status";

/// The meetings folder changed on disk.
pub const MEETINGS_CHANGED_EVENT: &str = "meetings-changed";
