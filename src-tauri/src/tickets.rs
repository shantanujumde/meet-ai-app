//! Listing and hand-creating tickets for the Tickets screen (TUR-102).
//!
//! A thin adapter over `store::ticket`: `store` owns the file format, this
//! module turns it into the shape the webview renders and picks the next id.
//! As everywhere else, a broken ticket file is a badge, not an error (SPEC §7).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use store::ticket::{self, Ticket};

use crate::error::UiError;
use crate::meetings;

/// How many times `create` bumps the id when another writer took it first.
const CREATE_ATTEMPTS: u32 = 5;

/// One ticket as the webview sees it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TicketSummary {
    pub id: String,
    pub title: String,
    /// `open`, `in_progress`, `done` or `dropped`; `None` when missing or
    /// not one of those.
    pub status: Option<String>,
    /// The meeting folder id it came from; `None` for a hand-made ticket.
    pub meeting: Option<String>,
    pub body: String,
    /// The file needs attention (bad YAML, missing id, odd status, ...).
    pub has_problems: bool,
}

/// Every ticket under the meetings root, newest first.
pub fn list() -> Result<Vec<TicketSummary>, UiError> {
    list_in(&meetings::root()?)
}

/// Add a ticket by hand. No meeting is attached.
pub fn create(title: &str, body: &str) -> Result<TicketSummary, UiError> {
    create_in(&meetings::root()?, title, body)
}

fn tickets_dir(root: &Path) -> PathBuf {
    root.join(store::TICKETS_DIR)
}

fn summarize(stem: &str, ticket: &Ticket) -> TicketSummary {
    TicketSummary {
        id: ticket.id().unwrap_or_else(|| stem.to_owned()),
        title: ticket.title().unwrap_or_else(|| stem.to_owned()),
        status: ticket.status().map(|s| s.as_str().to_owned()),
        meeting: ticket.meeting().filter(|m| !m.is_empty()),
        body: ticket.body.clone(),
        has_problems: !ticket.problems.is_empty(),
    }
}

/// The `.md` files in the tickets folder as `(file stem, path)`. A missing
/// folder is an empty list.
fn ticket_files(root: &Path) -> Result<Vec<(String, PathBuf)>, UiError> {
    let entries = match fs::read_dir(tickets_dir(root)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") || !path.is_file() {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            files.push((stem.to_owned(), path.clone()));
        }
    }
    Ok(files)
}

fn list_in(root: &Path) -> Result<Vec<TicketSummary>, UiError> {
    let mut rows: Vec<(Option<u32>, TicketSummary)> = Vec::new();
    for (stem, path) in ticket_files(root)? {
        let summary = match Ticket::read(&path) {
            Ok(ticket) => summarize(&stem, &ticket),
            // Unreadable file: still show it, flagged, so it is not silently lost.
            Err(_) => TicketSummary {
                id: stem.clone(),
                title: stem.clone(),
                status: None,
                meeting: None,
                body: String::new(),
                has_problems: true,
            },
        };
        rows.push((ticket::parse_id(&summary.id), summary));
    }
    // Newest (highest number) first; ids that are not numbers go last.
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    Ok(rows.into_iter().map(|(_, s)| s).collect())
}

fn create_in(root: &Path, title: &str, body: &str) -> Result<TicketSummary, UiError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(UiError::app(
            "ticket-title-empty",
            "Give the ticket a title before saving it.",
        ));
    }
    let dir = tickets_dir(root);
    fs::create_dir_all(&dir)?;

    let mut next = ticket_files(root)?
        .iter()
        .filter_map(|(stem, _)| ticket::parse_id(stem))
        .max()
        .map_or(1, |max| max.saturating_add(1));

    for _ in 0..CREATE_ATTEMPTS {
        let id = ticket::format_id(next);
        let path = dir.join(format!("{id}.md"));
        if path.exists() {
            next = next.saturating_add(1);
            continue;
        }
        let mut made = Ticket::new(&id, title, "");
        // Made by hand, so it came from no meeting.
        made.frontmatter.set_str("meeting", None);
        made.body = if body.is_empty() || body.ends_with('\n') {
            body.to_owned()
        } else {
            format!("{body}\n")
        };
        made.write(&path)?;
        return Ok(summarize(&id, &made));
    }
    Err(UiError::app(
        "ticket-id-taken",
        "Could not find a free ticket number. Please try again.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_root(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "meet-ai-tickets-{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    #[test]
    fn a_missing_tickets_folder_lists_nothing() {
        let root = temp_root("empty");
        assert!(list_in(&root).expect("list").is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn create_then_list_round_trips() {
        let root = temp_root("roundtrip");
        let made = create_in(&root, "  Fix the thing  ", "Some notes").expect("create");
        assert_eq!(made.id, "TICK-0001");
        assert_eq!(made.title, "Fix the thing");
        assert_eq!(made.status.as_deref(), Some("open"));
        assert_eq!(made.meeting, None);
        assert_eq!(made.body, "Some notes\n");
        assert!(!made.has_problems);

        let listed = list_in(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "TICK-0001");
        assert_eq!(listed[0].title, "Fix the thing");
        assert_eq!(listed[0].meeting, None);
        assert!(!listed[0].has_problems);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ids_increment_and_the_newest_is_first() {
        let root = temp_root("ids");
        let a = create_in(&root, "one", "").expect("a");
        let b = create_in(&root, "two", "").expect("b");
        assert_eq!((a.id.as_str(), b.id.as_str()), ("TICK-0001", "TICK-0002"));
        let ids: Vec<_> = list_in(&root)
            .expect("list")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["TICK-0002", "TICK-0001"]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_empty_title_is_rejected() {
        let root = temp_root("blank");
        let error = create_in(&root, "   ", "body").expect_err("rejected");
        assert_eq!(error.kind, "ticket-title-empty");
        assert!(list_in(&root).expect("list").is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_broken_ticket_is_listed_with_a_problem_flag() {
        let root = temp_root("broken");
        let dir = tickets_dir(&root);
        fs::create_dir_all(&dir).expect("dir");
        fs::write(
            dir.join("TICK-0007.md"),
            "---\ntitle: [unclosed\n---\nKeep.\n",
        )
        .expect("write");
        fs::write(dir.join("notes.txt"), "not a ticket").expect("write");
        let listed = list_in(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "TICK-0007");
        assert_eq!(listed[0].title, "TICK-0007");
        assert!(listed[0].has_problems);
        // The next id still moves past a broken file.
        assert_eq!(
            create_in(&root, "next", "").expect("create").id,
            "TICK-0008"
        );
        fs::remove_dir_all(&root).ok();
    }
}
