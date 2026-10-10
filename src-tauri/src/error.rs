//! The one error shape that crosses the IPC boundary.
//!
//! **Two enums, not one.** `stt::Error` and `modelfetch::Error` are deliberately
//! separate types (see the comment on `modelfetch::Error`) and the frontend must
//! keep them apart too — a checksum mismatch and a dead sidecar need different
//! words and a different button. So the wire shape carries a `domain` alongside
//! the `kind` rather than flattening both enums into one tag namespace.
//!
//! **The UI branches on `domain` + `kind`, never on `message`.** `message` is
//! whatever the Rust error printed, shown verbatim next to the button so the
//! user can copy it into a bug report. Matching on it would turn every reworded
//! error string into a silently broken screen.

use serde::Serialize;

/// An error as the webview sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Which error enum this came from.
    pub domain: ErrorDomain,
    /// The stable variant tag. Safe to `switch` on.
    pub kind: &'static str,
    /// The error's own sentence. Display it; do not parse it.
    pub message: String,
}

/// [`UiError::domain`]: an enum, so the window's type is the closed union
/// it branches on (TUR-173) rather than a `string`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ErrorDomain {
    /// `stt::Error`.
    Stt,
    /// `modelfetch::Error`.
    Model,
    /// The app shell itself, and the crates it maps into it.
    App,
}

#[cfg(test)]
impl ErrorDomain {
    /// The name on the wire: `stt`, `model` or `app`. For tests to compare with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stt => "stt",
            Self::Model => "model",
            Self::App => "app",
        }
    }
}

impl UiError {
    /// An error raised by the app shell itself rather than by a domain crate.
    pub fn app(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            domain: ErrorDomain::App,
            kind,
            message: message.into(),
        }
    }
}

impl From<stt::Error> for UiError {
    fn from(error: stt::Error) -> Self {
        // Matching the variant, not the string. Adding a variant to stt::Error
        // makes this arm list fail to compile, which is the point.
        let kind = match &error {
            stt::Error::EngineUnavailable(_) => "engine-unavailable",
            stt::Error::Engine(_) => "engine",
            stt::Error::StreamingUnsupported(_) => "streaming-unsupported",
            stt::Error::Sidecar(_) => "sidecar",
            stt::Error::ModelMissing(_) => "model-missing",
            stt::Error::Segments(_) => "segments",
            stt::Error::Wav(_) => "wav",
            stt::Error::Io(_) => "io",
        };
        Self {
            domain: ErrorDomain::Stt,
            kind,
            message: error.to_string(),
        }
    }
}

impl From<modelfetch::Error> for UiError {
    fn from(error: modelfetch::Error) -> Self {
        // modelfetch already publishes `kind()` precisely so callers do not have
        // to read the message. Use it.
        let kind = match error.kind() {
            modelfetch::ErrorKind::Download => "download",
            modelfetch::ErrorKind::Checksum => "checksum",
        };
        Self {
            domain: ErrorDomain::Model,
            kind,
            message: error.to_string(),
        }
    }
}

impl From<store::Error> for UiError {
    fn from(error: store::Error) -> Self {
        // Kept in the `app` domain, with the same kinds the old in-shell reader
        // produced (`io`, `bad-meeting-id`), so moving the reader into `store`
        // changed no error the webview receives. A separate `store` domain is
        // the cleaner shape, but [`ErrorDomain`] is the closed union the
        // window switches on; widening it belongs with the UI that needs to tell
        // the cases apart (TUR-102), not with the move. The two new kinds fall
        // through to the generic copy until then.
        match error {
            // `store::Error::Io`'s own message is the generic "could not read
            // or write the meetings folder"; the OS error is what to show.
            store::Error::Io(io) => io.into(),
            store::Error::BadId(id) => Self::app(
                "bad-meeting-id",
                format!("{id:?} is not a meeting folder name."),
            ),
            error @ store::Error::Frontmatter { .. } => Self::app("frontmatter", error.to_string()),
            error @ store::Error::Index(_) => Self::app("index", error.to_string()),
        }
    }
}

impl From<crate::config::ConfigError> for UiError {
    fn from(error: crate::config::ConfigError) -> Self {
        use crate::config::ConfigError;
        // `app` domain like the other shell errors. `unknown-harness` is its
        // own kind so the Setup screen can point at the agent picker.
        let kind = match &error {
            ConfigError::UnknownHarness(_) => "unknown-harness",
            ConfigError::Invalid(_) => "invalid-config",
            ConfigError::Io(_) => "io",
            ConfigError::Root(_) => "root",
        };
        // Already a UI error (no meetings folder, say): keep its own kind.
        if let ConfigError::Root(error) = error {
            return error;
        }
        Self::app(kind, error.to_string())
    }
}

impl From<std::io::Error> for UiError {
    fn from(error: std::io::Error) -> Self {
        Self::app("io", error.to_string())
    }
}

/// Run a command's blocking work — disk, a subprocess, the recorder — on
/// Tauri's blocking pool. The one copy every command module uses.
///
/// A plain `#[tauri::command]` runs on the main thread — the one AppKit draws
/// the window on — so every disk-touching command used to freeze the app while
/// it worked. `list_meetings` reads the WAV header of every meeting, and
/// `change_meetings_folder` can fall back to copying a whole folder tree across
/// volumes, which takes minutes. `#[tauri::command(async)]` alone would only
/// move that onto a tokio worker, and parking a worker for minutes starves the
/// other async commands, so the work goes to the pool built for blocking.
///
/// The closure must be `'static`, which is why these commands take an
/// `AppHandle` and look managed state up inside rather than borrowing a
/// `State<'_, T>` across the hop. A closure that returns a `Result` comes back
/// as `Result<Result<_>>`; callers flatten it with `?`. A task that panicked
/// or was cancelled is the `task-failed` kind.
pub(crate) async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_enums_stay_in_separate_domains() {
        let engine: UiError = stt::Error::EngineUnavailable("nothing here".into()).into();
        let checksum: UiError = modelfetch::Error::Checksum {
            model: "small.en-q5_1",
            expected: "aaa",
            actual: "bbb".into(),
        }
        .into();

        assert_eq!(
            (engine.domain.as_str(), engine.kind),
            ("stt", "engine-unavailable")
        );
        assert_eq!(
            (checksum.domain.as_str(), checksum.kind),
            ("model", "checksum")
        );

        // Same tag in two domains must not collide into one UI branch.
        let download: UiError = modelfetch::Error::Download("timed out".into()).into();
        assert_ne!(
            (engine.domain.as_str(), engine.kind),
            (download.domain.as_str(), download.kind)
        );
    }

    #[test]
    fn blocking_work_comes_back_and_a_panic_is_task_failed() {
        let answer = tauri::async_runtime::block_on(on_blocking_pool(|| 6 * 7)).unwrap();
        assert_eq!(answer, 42);

        let error = tauri::async_runtime::block_on(on_blocking_pool(|| -> u8 {
            panic!("the work fell over");
        }))
        .unwrap_err();
        assert_eq!((error.domain.as_str(), error.kind), ("app", "task-failed"));
    }

    #[test]
    fn the_message_reaches_the_ui_verbatim() {
        let error = stt::Error::Sidecar("exit status 2".into());
        let expected = error.to_string();
        let ui: UiError = error.into();
        assert_eq!(ui.message, expected);
    }
}
