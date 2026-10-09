//! The error a recording that ended on its own leaves on the idle status
//! (TUR-97). Moved out of `recording.rs` (TUR-146) so that file does not grow.

use crate::error::UiError;

/// The tick failure that ended the recording, and whether closing its files
/// worked after that (`stop_error`).
pub(super) fn interrupted(message: &str, stop_error: Option<&str>) -> UiError {
    let body = match stop_error {
        None => format!(
            "The recording stopped because of an error ({message}). The audio recorded up to \
             that point was saved."
        ),
        Some(stop_message) => format!(
            "The recording stopped because of an error ({message}), and finishing its files also \
             failed ({stop_message})."
        ),
    };
    UiError::app("recording-interrupted", body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupted_recording_says_whether_its_files_were_saved() {
        let saved = interrupted("mic fsync: disk full", None);
        assert_eq!(saved.kind, "recording-interrupted");
        assert!(saved.message.contains("(mic fsync: disk full)"));
        assert!(saved.message.contains("was saved"), "{}", saved.message);

        let lost = interrupted("tick", Some("stop"));
        assert!(lost.message.contains("(tick)") && lost.message.contains("(stop)"));
        assert!(!lost.message.contains("was saved"), "{}", lost.message);
    }
}
