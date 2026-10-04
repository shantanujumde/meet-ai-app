//! Reading the meetings folder for the app shell.
//!
//! A thin adapter over `crates/store`, which owns markdown read/write,
//! frontmatter and (from Phase 3c) the FTS index (SPEC §5). This module turns
//! a loaded [`store::folder::MeetingFolder`] into the shapes the webview
//! renders, and keeps the one thing `store` deliberately does not know about:
//! *where* the meetings root is, and moving it.
//!
//! Two rules carried over from the spec, because getting them wrong here is
//! invisible until a recording is lost:
//!
//! * **`transcript.md` is read-only to the app shell.** L7 makes markdown the
//!   source of truth and §3.4 makes it append-only. Nothing in this file opens
//!   it for writing. (The one rewrite, sorting it by time when a recording
//!   stops, is `live_transcript`'s, SPEC A19.)
//! * **A malformed file is a badge, not an error (SPEC §7).** `store` loads
//!   every file it can and reports the rest as problems; the unparsed-line
//!   count below is one of them.
//!
//! It also tells a finished meeting from one whose recording was cut short
//! (TUR-97) — see [`RecordingState`] for the name and [`classify_audio`] for
//! the rule.

mod list;
mod root;
#[cfg(test)]
mod tests;
mod view;

pub use self::list::{Live, MeetingList, list, recover_interrupted_audio};
pub use self::root::{change_root, root};
pub use self::view::{MeetingDetail, detail, rename, write_notes};
// `recording.rs`'s tests check that the list parser reads back what the
// recorder writes.
#[cfg(test)]
pub(crate) use store::folder_name::split_folder_name;
