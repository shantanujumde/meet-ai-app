//! Issues a Sync run created but could not save, kept on disk (TUR-21).
//!
//! [`super::save::Unsaved`] keeps such an issue so Retry saves it instead of
//! creating a second one. This file makes that survive a restart: a quit
//! app must not forget an issue that already exists in the tracker.
//!
//! It lives at `<root>/.app/unsaved-syncs.json`. The app writes nothing
//! outside the meetings root (SPEC L10), and the kept issues belong to the
//! tickets in that root, so the file moves with it.
//!
//! The format is one small JSON object:
//!
//! ```json
//! {
//!   "version": 1,
//!   "kept": [
//!     { "meeting": "2026-09-01-1430-standup", "ticket": "TICK-0001",
//!       "tracker": "linear", "fingerprint": "<sha256 hex>",
//!       "external_id": "ENG-42",
//!       "external_url": "https://linear.app/acme/issue/ENG-42" }
//!   ]
//! }
//! ```
//!
//! `meeting` is absent for a shared ticket. `fingerprint` is the ticket
//! file as it was when its sync started ([`Fingerprint`]).
//!
//! This is never the ticket file itself: the ticket is the thing that could
//! not be written. A single entry that fails its checks (a hand edit) is
//! skipped with a warning. A file that cannot be read at all is an error, so
//! it is never ignored or written over while it may still hold issues.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use prompts::push_ticket::parse_sync_reply;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::save::{Created, Fingerprint, Key, key};

/// File name inside `<root>/.app/`.
pub(super) const FILE: &str = "unsaved-syncs.json";

/// The format this build reads and writes.
const VERSION: u32 = 1;

/// The whole file.
#[derive(Serialize, Deserialize)]
struct Kept {
    version: u32,
    /// Read one by one, so one bad entry does not lose the others.
    #[serde(default)]
    kept: Vec<Value>,
}

/// One kept issue.
#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    meeting: Option<String>,
    ticket: String,
    tracker: String,
    fingerprint: String,
    external_id: String,
    external_url: String,
}

/// `<root>/.app/unsaved-syncs.json`
pub(super) fn path(root: &Path) -> PathBuf {
    meeting_format::layout::app_dir(root).join(FILE)
}

/// What an earlier run of the app kept under `root`. A missing file is an
/// empty list. A file that cannot be read, is not JSON or has a `version`
/// this build does not know is an error: it may hold issues that exist, so
/// the caller must neither ignore it nor write over it. Single entries that
/// fail the checks are skipped.
pub(super) fn load(root: &Path) -> io::Result<Vec<(Key, Created)>> {
    let bytes = match fs::read(path(root)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let file: Kept = serde_json::from_slice(&bytes)?;
    if file.version != VERSION {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("version {} is not one this app knows", file.version),
        ));
    }
    Ok(file.kept.into_iter().filter_map(entry).collect())
}

/// One entry, if it passes the checks. The link gets written into a ticket,
/// so it gets the same check as a fresh agent reply: a hand-edited file must
/// not slip in a link that is not `https://`.
fn entry(value: Value) -> Option<(Key, Created)> {
    let entry: Entry = match serde_json::from_value(value) {
        Ok(entry) => entry,
        Err(error) => {
            tracing::warn!(%error, "skipping a kept sync that could not be parsed");
            return None;
        }
    };
    if store::ticket::parse_id(&entry.ticket).is_none() {
        tracing::warn!(ticket = %entry.ticket, "skipping a kept sync with a bad ticket id");
        return None;
    }
    if entry.tracker.trim().is_empty() {
        tracing::warn!(ticket = %entry.ticket, "skipping a kept sync with no tracker");
        return None;
    }
    let reply = serde_json::json!({
        "external_id": entry.external_id,
        "external_url": entry.external_url,
    });
    let Some(synced) = parse_sync_reply(&reply) else {
        tracing::warn!(ticket = %entry.ticket, "skipping a kept sync with a bad issue key or link");
        return None;
    };
    let created = Created {
        tracker: entry.tracker,
        synced,
        ticket: Fingerprint::from_hex(entry.fingerprint),
    };
    Some((key(&entry.ticket, entry.meeting.as_deref()), created))
}

/// Writes all `kept` issues to the file, replacing it. Nothing kept removes
/// the file. Sorted by key, so the same issues always make the same file.
pub(super) fn store<'a>(
    root: &Path,
    kept: impl IntoIterator<Item = (&'a Key, &'a Created)>,
) -> io::Result<()> {
    let path = path(root);
    let mut kept: Vec<_> = kept.into_iter().collect();
    if kept.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    kept.sort_by(|a, b| a.0.cmp(b.0));
    let entries = kept
        .into_iter()
        .map(|((meeting, ticket), created)| {
            serde_json::to_value(Entry {
                meeting: meeting.clone(),
                ticket: ticket.clone(),
                tracker: created.tracker.clone(),
                fingerprint: created.ticket.as_hex().to_owned(),
                external_id: created.synced.external_id.clone(),
                external_url: created.synced.external_url.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let file = Kept {
        version: VERSION,
        kept: entries,
    };
    let mut bytes = serde_json::to_vec_pretty(&file)?;
    bytes.push(b'\n');
    // Only `.app` itself: a root that is gone (an unplugged drive, a folder
    // moved away) must fail here, not be made again empty.
    match fs::create_dir(meeting_format::layout::app_dir(root)) {
        Err(error) if error.kind() != ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    meeting_format::write_atomic(&path, &bytes)
}

#[cfg(test)]
mod tests {
    use prompts::push_ticket::Synced;

    use super::*;

    fn created(id: &str) -> Created {
        Created {
            tracker: "linear".to_owned(),
            synced: Synced {
                external_id: id.to_owned(),
                external_url: format!("https://linear.app/acme/issue/{id}"),
            },
            ticket: Fingerprint::of_bytes(id.as_bytes()),
        }
    }

    fn write(root: &Path, text: &str) {
        fs::create_dir_all(root.join(".app")).unwrap();
        fs::write(path(root), text).unwrap();
    }

    #[test]
    fn path_is_under_the_app_folder() {
        let root = Path::new("/meetings");
        assert!(path(root).ends_with(".app/unsaved-syncs.json"));
    }

    #[test]
    fn round_trip() {
        let root = tempfile::tempdir().unwrap();
        let shared = (key("TICK-0002", None), created("ENG-2"));
        let meeting = (
            key("TICK-0001", Some("2026-09-01-1430-standup")),
            created("ENG-1"),
        );
        let kept = [meeting.clone(), shared.clone()];
        store(root.path(), kept.iter().map(|(k, c)| (k, c))).unwrap();

        let mut loaded = load(root.path()).unwrap();
        loaded.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(loaded, vec![shared, meeting]);
    }

    #[test]
    fn stored_file_is_sorted_and_versioned() {
        let root = tempfile::tempdir().unwrap();
        let b = (key("TICK-0002", Some("b")), created("ENG-2"));
        let a = (key("TICK-0001", Some("a")), created("ENG-1"));
        store(root.path(), [(&b.0, &b.1), (&a.0, &a.1)]).unwrap();

        let text = fs::read_to_string(path(root.path())).unwrap();
        let file: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["version"], 1);
        assert_eq!(file["kept"][0]["ticket"], "TICK-0001");
        assert_eq!(file["kept"][1]["ticket"], "TICK-0002");
    }

    #[test]
    fn missing_file_is_empty() {
        let root = tempfile::tempdir().unwrap();
        assert!(load(root.path()).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_file_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "{ not json");
        assert!(load(root.path()).is_err());
    }

    #[test]
    fn an_unknown_version_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            r#"{"version": 2, "kept": [{"ticket": "TICK-0001", "tracker": "linear",
                "fingerprint": "ab", "external_id": "ENG-1",
                "external_url": "https://linear.app/acme/issue/ENG-1"}]}"#,
        );
        assert!(load(root.path()).is_err());
    }

    #[test]
    fn bad_entries_are_skipped_and_good_ones_kept() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path(),
            r#"{"version": 1, "kept": [
                {"ticket": "TICK-0001", "tracker": "linear", "fingerprint": "ab",
                 "external_id": "ENG-1", "external_url": "http://linear.app/acme/issue/ENG-1"},
                {"ticket": "TICK-0002", "tracker": "linear", "fingerprint": "ab",
                 "external_id": "ENG-2", "external_url": "javascript:alert(1)"},
                {"ticket": "../x", "tracker": "linear", "fingerprint": "ab",
                 "external_id": "ENG-3", "external_url": "https://linear.app/acme/issue/ENG-3"},
                {"ticket": "TICK-0004"},
                {"meeting": "", "ticket": "TICK-0005", "tracker": "linear", "fingerprint": "ab",
                 "external_id": "ENG-5", "external_url": "https://linear.app/acme/issue/ENG-5"}
            ]}"#,
        );
        let loaded = load(root.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        let (found, kept) = &loaded[0];
        assert_eq!(found, &key("TICK-0005", None));
        assert_eq!(kept.synced.external_id, "ENG-5");
        assert_eq!(kept.ticket.as_hex(), "ab");
    }

    #[test]
    fn storing_nothing_removes_the_file() {
        let root = tempfile::tempdir().unwrap();
        let one = (key("TICK-0001", None), created("ENG-1"));
        store(root.path(), [(&one.0, &one.1)]).unwrap();
        assert!(path(root.path()).exists());

        store(root.path(), []).unwrap();
        assert!(!path(root.path()).exists());
        // Nothing to remove is fine too.
        store(root.path(), []).unwrap();
    }
}
