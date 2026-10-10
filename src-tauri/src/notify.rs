//! Desktop notifications for a recording that did not start or stopped on its
//! own, on a surface that does not need the window to be open.

use tauri_plugin_notification::NotificationExt as _;

/// Show a desktop notification, logging when it could not be shown. The one
/// place a notification is posted (TUR-176): every caller only says what.
/// `false` when it was not shown.
pub fn post<R: tauri::Runtime>(app: &tauri::AppHandle<R>, title: &str, body: &str) -> bool {
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, title, "could not show a notification");
            false
        }
    }
}

/// Tell the user why a ⌘⇧R or menu-bar toggle failed — a start refused, or a
/// stop that could not close cleanly (`stopping`) — on a surface that does not
/// need the window to be open. The window hears it too, from the idle status
/// on the recording state event that carries the refusal (`recording::Status::error`,
/// set by the recorder or, for a folder-move refusal, by `folder_move`) — the
/// notification alone is silent whenever meet-ai may not post them, which made
/// a refused ⌘⇧R look like it had done nothing at all.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn refusal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    stopping: bool,
    error: &crate::error::UiError,
) {
    post(app, refusal_title(stopping), &error.message);
}

/// The notification title for a toggle that failed, by which way it was going.
/// A failed stop still ends the recording — the recorder is back at `Idle`
/// with the reason on its status — it just could not close the files cleanly,
/// so "did not start recording" there was simply wrong.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn refusal_title(stopping: bool) -> &'static str {
    if stopping {
        "meet-ai had a problem stopping the recording"
    } else {
        "meet-ai did not start recording"
    }
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::refusal_title;

    #[test]
    fn a_failed_stop_is_not_titled_as_a_failed_start() {
        assert_eq!(refusal_title(false), "meet-ai did not start recording");
        assert!(refusal_title(true).contains("stopping"));
        assert!(!refusal_title(true).contains("start"));
    }
}

/// Tell the user a recording stopped on its own, on a surface that does not
/// need the window to be open — the same reasoning as [`refusal`] for the shortcut.
pub fn interrupted(app: &tauri::AppHandle, message: &str) {
    post(app, "meet-ai stopped recording", message);
}

/// Tell the user a meeting's notes are written, when the window is not in
/// front to show it (TUR-10).
pub fn notes_ready(app: &tauri::AppHandle, title: &str, tasks: u32) {
    post(app, "Notes are ready", &notes_ready_body(title, tasks));
}

/// "Standup: the notes and 2 tasks are written."
fn notes_ready_body(title: &str, tasks: u32) -> String {
    match tasks {
        0 => format!("{title}: the notes are written."),
        1 => format!("{title}: the notes and 1 task are written."),
        n => format!("{title}: the notes and {n} tasks are written."),
    }
}

/// Tell the user live transcription stopped while the meeting is still
/// recording (TUR-161). A ⌘⇧R recording usually has its window hidden, so
/// the pane saying so is not enough; a failed tick is told the same way
/// ([`interrupted`]). `detail` is the sentence the pane shows. Posted once
/// per meeting: only the first failure is reported.
pub fn transcription_failed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, detail: &str) {
    post(app, "meet-ai stopped transcribing", detail);
}
