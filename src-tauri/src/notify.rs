//! Desktop notifications for a recording that did not start or stopped on its
//! own, on a surface that does not need the window to be open.

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
    use tauri_plugin_notification::NotificationExt as _;

    if let Err(notify_error) = app
        .notification()
        .builder()
        .title(refusal_title(stopping))
        .body(&error.message)
        .show()
    {
        tracing::warn!(%notify_error, "could not show the refusal notification either");
    }
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
    use tauri_plugin_notification::NotificationExt as _;

    if let Err(error) = app
        .notification()
        .builder()
        .title("meet-ai stopped recording")
        .body(message)
        .show()
    {
        tracing::warn!(%error, "could not show the interrupted-recording notification");
    }
}
