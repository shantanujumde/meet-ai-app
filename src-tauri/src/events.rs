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

/// A meeting's notes run changed state (TUR-10): `agent_run::Status`.
pub const AGENT_RUN_STATUS_EVENT: &str = "agent-run://status";

/// A meeting looks like it started and meet-ai is asking whether to record it
/// (TUR-27): `detection::notify::Prompt`, the `Signal` and the reason.
pub const DETECTION_PROMPT_EVENT: &str = "detection://prompt";

/// A quit (⌘Q, the menu-bar Quit) came in while recording, and Rust is
/// holding it until the window's "Stop recording and quit?" is answered
/// (TUR-76). No payload.
pub const QUIT_CONFIRM_EVENT: &str = "app://confirm-quit";

/// Rust wants the window on a screen: the menu bar's "Open brief" and
/// "Calendar not connected" (TUR-77). `lifecycle::NavigateTo`.
pub const NAVIGATE_EVENT: &str = "app://navigate";

/// The prompt popup window (TUR-59, Windows and Linux) has a prompt to show:
/// `detection::popup::PopupPrompt`. Sent to that window only.
pub const PROMPT_POPUP_EVENT: &str = "prompt-popup://show";

/// A user hook failed (TUR-63): `hooks::app::HookFailed`. The meeting view
/// shows a small "Hook failed" note.
pub const HOOK_FAILED_EVENT: &str = "hook://failed";

/// Whether the recording window shows "No headphones" (TUR-65):
/// `headphone_warning::HeadphoneWarning`. Sent when a recording starts, when
/// the output changes between speakers and headphones, and when it stops.
pub const HEADPHONE_WARNING_EVENT: &str = "headphones://warning";

/// The meetings folder cannot be watched, or can be again (TUR-134):
/// `watch::WatchProblem`. The Meetings page shows a note while it is set.
pub const MEETINGS_WATCH_PROBLEM_EVENT: &str = "meetings-watch://problem";

/// A ticket on the Tickets page changed how far it is in being sent to the
/// tracker on its own (TUR-113): `sync::auto::TicketSyncStatus`.
pub const TICKET_SYNC_EVENT: &str = "ticket-sync://status";
