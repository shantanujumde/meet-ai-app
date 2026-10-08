//! The Action Items section [`super::write()`] fills: every task of the
//! meeting, suggested or approved (SPEC A26, TUR-113).

use std::path::{Path, PathBuf};

use super::{Planned, text};
use crate::TICKETS_DIR;
use crate::ticket::{self, Ticket};

/// One line per ticket of the meeting, in number order: the suggestions in
/// its own `tickets/` folder, and the approved ones in the root's `tickets/`
/// that name it. The tickets this run wrote use the task's words; the rest
/// are read back from disk.
pub(super) fn action_items(
    root: &Path,
    meeting_id: &str,
    tickets_dir: &Path,
    writes: &[Planned<'_>],
) -> Vec<String> {
    let mut found: Vec<(u32, String, PathBuf)> = ticket_files(tickets_dir);
    for (n, id, path) in ticket_files(&root.join(TICKETS_DIR)) {
        let theirs = Ticket::read(&path)
            .ok()
            .and_then(|t| t.meeting())
            .is_some_and(|m| m == meeting_id);
        if theirs && !found.iter().any(|(_, known, _)| *known == id) {
            found.push((n, id, path));
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|(_, id, path)| {
            if let Some(planned) = writes.iter().find(|p| p.id == id) {
                let task = text::clean(planned.task);
                return text::action_line(
                    &id,
                    &task.title,
                    task.owner.as_deref(),
                    task.due.as_deref(),
                );
            }
            let found = Ticket::read(&path).ok();
            let title = found.as_ref().and_then(Ticket::title).unwrap_or_default();
            let owner = found.as_ref().and_then(Ticket::assignee);
            let due = found.as_ref().and_then(|t| text::due_in(&t.body));
            text::action_line(&id, &title, owner.as_deref(), due.as_deref())
        })
        .collect()
}

/// The `TICK-NNNN.md` files in `dir` as `(number, id, path)`, unsorted. A
/// folder that cannot be listed has none.
pub(super) fn ticket_files(dir: &Path) -> Vec<(u32, String, PathBuf)> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let id = name.strip_suffix(".md")?.to_owned();
                    Some((ticket::parse_id(&id)?, id, entry.path()))
                })
                .collect()
        })
        .unwrap_or_default()
}
