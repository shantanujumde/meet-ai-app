//! Where the recorder is, as the webview names it.

use serde::Serialize;

/// Where the recorder is right now.
///
/// `Starting` and `Stopping` are not decoration: opening the tap (and, on
/// start, the SPEC §8.1 permission measurement ahead of it) and flushing the
/// last WAV header both take long enough to see, and a shortcut pressed twice
/// in that window must be ignored rather than queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Starting,
    Recording,
    Stopping,
}
