//! On-disk storage for meet-ai.
//!
//! Phase 3 territory (SPEC §5). Nothing is implemented yet.
//!
//! Two rules from the spec are worth reading before adding anything here,
//! because both are easy to violate by accident:
//!
//! * **L7 — markdown is the truth.** `index.db` is derived and must be safe to
//!   delete. The Phase 3 exit gate is literally "delete `index.db`, everything
//!   still works after a rescan".
//! * **SPEC §4 — the watcher must suppress this process's own writes.** The app
//!   writes `notes.md` while the user types in it. Without suppression the
//!   watcher fires, the app reloads, and the cursor jumps mid-sentence. Agent
//!   writes are *not* suppressed.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::time::Duration;

/// How long a path this process wrote stays ignored by the watcher.
///
/// SPEC §4. Longer than the `notify` debounce, short enough that a real external
/// edit landing right after ours is not swallowed.
pub const SELF_WRITE_SUPPRESSION: Duration = Duration::from_millis(750);

/// How long `notify` events are coalesced before the watcher acts on them.
///
/// Agent writes arrive as a burst of files; debouncing turns that into one
/// reload instead of a dozen.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);

/// Everything that can go wrong reading or writing the meetings folder.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file under the meetings root could not be read or written.
    #[error("could not read or write the meetings folder")]
    Io(#[from] std::io::Error),

    /// A `meeting.md` frontmatter block was not valid YAML.
    ///
    /// Frontmatter round-trips must preserve unknown keys (SPEC §2.3), so this
    /// is only raised when the YAML itself will not parse.
    #[error("the frontmatter in {path} is not valid YAML: {detail}")]
    Frontmatter { path: String, detail: String },

    /// The derived index could not be opened, migrated or queried.
    ///
    /// Always recoverable: delete `index.db` and rescan.
    #[error("the search index is unusable and needs rebuilding")]
    Index(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_is_available() {
        // L7 makes index.db a full-text search index, and rusqlite 0.40 has no
        // `fts5` feature flag to ask for it. This proves the bundled
        // amalgamation was compiled with FTS5 on, so Phase 3 does not discover
        // otherwise.
        let db = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        db.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body);")
            .expect("FTS5 is missing from the bundled SQLite build");
    }

    #[test]
    fn suppression_outlives_the_debounce_window() {
        // If this ever inverts, a self-write echo arrives after suppression has
        // already expired and the user's cursor jumps. Guard it with a test
        // rather than a comment.
        assert!(SELF_WRITE_SUPPRESSION > WATCH_DEBOUNCE);
    }
}
