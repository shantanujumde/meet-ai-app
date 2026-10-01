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
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    /// Which error enum this came from: `stt`, `model`, or `app`.
    pub domain: &'static str,
    /// The stable variant tag. Safe to `switch` on.
    pub kind: &'static str,
    /// The error's own sentence. Display it; do not parse it.
    pub message: String,
}

impl UiError {
    /// An error raised by the app shell itself rather than by a domain crate.
    pub fn app(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            domain: "app",
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
            domain: "stt",
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
            domain: "model",
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
        // the cleaner shape, but `UiError['domain']` in src/ipc/types.ts is a
        // closed union; widening it belongs with the UI that needs to tell
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

impl From<std::io::Error> for UiError {
    fn from(error: std::io::Error) -> Self {
        Self::app("io", error.to_string())
    }
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

        assert_eq!((engine.domain, engine.kind), ("stt", "engine-unavailable"));
        assert_eq!((checksum.domain, checksum.kind), ("model", "checksum"));

        // Same tag in two domains must not collide into one UI branch.
        let download: UiError = modelfetch::Error::Download("timed out".into()).into();
        assert_ne!(
            (engine.domain, engine.kind),
            (download.domain, download.kind)
        );
    }

    #[test]
    fn the_message_reaches_the_ui_verbatim() {
        let error = stt::Error::Sidecar("exit status 2".into());
        let expected = error.to_string();
        let ui: UiError = error.into();
        assert_eq!(ui.message, expected);
    }
}
