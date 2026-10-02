//! The words [`super::write()`] puts in files, kept to the copy-prompt
//! template's format (`crates/prompts/templates/wrap-up.md`) so a meeting reads
//! the same whichever path made its notes.

use prompts::notes::{Notes, Task};

use crate::meeting::Meeting;
use crate::ticket::Ticket;

/// What an empty section says, as on the copy-prompt path.
const NONE: &str = "None.\n";

/// The title a task with a blank one gets, when its details are blank too.
const UNTITLED: &str = "Untitled task";

/// How much of the details stands in for a blank title, in characters.
const TITLE_FROM_DETAILS_CHARS: usize = 80;

/// The task's fields with blanks read as "not said", so an owner of `""`
/// never becomes `assignee: ""` and a due of `""` never becomes `Due: .`.
pub(super) struct Cleaned {
    pub title: String,
    pub details: String,
    pub owner: Option<String>,
    pub due: Option<String>,
}

pub(super) fn clean(task: &Task) -> Cleaned {
    let details = task.details.trim().to_owned();
    let mut title = fold(&task.title);
    if title.is_empty() {
        let first_line = details.lines().next().map(fold).unwrap_or_default();
        title = first_line.chars().take(TITLE_FROM_DETAILS_CHARS).collect();
    }
    if title.is_empty() {
        title = UNTITLED.to_owned();
    }
    Cleaned {
        title,
        details,
        owner: said(task.owner.as_deref()),
        due: said(task.due.as_deref()),
    }
}

fn said(value: Option<&str>) -> Option<String> {
    value.map(fold).filter(|v| !v.is_empty())
}

/// All whitespace runs, line breaks included, as one space.
fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The ticket file for one task (§3.3).
pub(super) fn ticket_for(id: &str, meeting_id: &str, task: &Task) -> Ticket {
    let task = clean(task);
    let mut made = Ticket::new(id, &task.title, meeting_id);
    made.frontmatter.set_str("assignee", task.owner.as_deref());
    // `transcript_ref` is set by the caller from the raw task: the schema
    // already pinned it to `HH:MM:SS`.
    let paragraphs: Vec<String> = [
        Some(task.details).filter(|d| !d.is_empty()),
        task.due.map(|due| format!("Due: {due}.")),
    ]
    .into_iter()
    .flatten()
    .collect();
    made.body = if paragraphs.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", paragraphs.join("\n\n"))
    };
    made
}

/// The `Due: X.` paragraph [`ticket_for`] writes, read back.
pub(super) fn due_in(body: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let due = line.trim().strip_prefix("Due: ")?.strip_suffix('.')?;
        said(Some(due))
    })
}

/// Fill the four sections. Bodies follow the file's line ending, so a
/// `meeting.md` saved on Windows does not come out mixed.
pub(super) fn fill_sections(meeting: &mut Meeting, notes: &Notes, actions: &[String]) {
    let eol = meeting.eol();
    let crlf = |body: String| {
        if eol == "\n" {
            body
        } else {
            body.replace('\n', eol)
        }
    };
    meeting.set_section("Summary", &crlf(paragraph(&notes.summary)));
    meeting.set_section("Decisions", &crlf(bullets(&notes.decisions)));
    meeting.set_section("Action Items", &crlf(bullets(actions)));
    meeting.set_section("Open Questions", &crlf(bullets(&notes.open_questions)));
}

/// `TICK-0001: Move sessions to Redis (Priya, due Friday)`.
pub(super) fn action_line(id: &str, title: &str, owner: Option<&str>, due: Option<&str>) -> String {
    let mut line = format!("{id}: {title}");
    let due = due.map(|d| format!("due {d}"));
    let extra: Vec<String> = owner.map(str::to_owned).into_iter().chain(due).collect();
    if !extra.is_empty() {
        line.push_str(&format!(" ({})", extra.join(", ")));
    }
    line
}

/// Free text for a section body. A line that starts with `## ` would split
/// the section on the next read, so its `#` is escaped.
fn paragraph(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return NONE.to_owned();
    }
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("## ") {
            out.push('\\');
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One `- item` line per non-empty item, each folded onto a single line;
/// [`NONE`] when there are none.
fn bullets(items: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        let folded = fold(item);
        if !folded.is_empty() {
            out.push_str("- ");
            out.push_str(&folded);
            out.push('\n');
        }
    }
    if out.is_empty() {
        out.push_str(NONE);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(title: &str, details: &str, owner: Option<&str>, due: Option<&str>) -> Task {
        Task {
            title: title.to_owned(),
            details: details.to_owned(),
            owner: owner.map(str::to_owned),
            due: due.map(str::to_owned),
            transcript_ref: "00:00:05".to_owned(),
        }
    }

    #[test]
    fn a_heading_in_the_summary_cannot_start_a_section() {
        assert_eq!(paragraph("ok\n## Evil\n"), "ok\n\\## Evil\n");
    }

    #[test]
    fn bullets_fold_lines_and_skip_blanks() {
        let items = ["one\ntwo".to_owned(), "  ".to_owned(), "three".to_owned()];
        assert_eq!(bullets(&items), "- one two\n- three\n");
        assert_eq!(bullets(&[]), "None.\n");
        assert_eq!(paragraph(" "), "None.\n");
    }

    #[test]
    fn action_lines_leave_out_what_was_not_said() {
        assert_eq!(
            action_line("TICK-0001", "Ship it", Some("Priya"), Some("Friday")),
            "TICK-0001: Ship it (Priya, due Friday)"
        );
        assert_eq!(
            action_line("TICK-0002", "Ship it", None, None),
            "TICK-0002: Ship it"
        );
    }

    #[test]
    fn blank_owner_and_due_read_as_not_said() {
        let made = ticket_for(
            "TICK-0001",
            "m",
            &task("Ship", "Do it", Some(" "), Some("")),
        );
        assert_eq!(made.assignee(), None);
        assert_eq!(made.body, "\nDo it\n");
    }

    #[test]
    fn a_blank_title_falls_back_to_the_details_then_to_untitled() {
        assert_eq!(
            clean(&task(" ", "Fix the\nlogin page", None, None)).title,
            "Fix the"
        );
        assert_eq!(clean(&task("", " ", None, None)).title, "Untitled task");
    }

    #[test]
    fn the_due_date_reads_back_from_the_body() {
        let made = ticket_for(
            "TICK-0001",
            "m",
            &task("Ship", "Do it", None, Some("Friday")),
        );
        assert_eq!(made.body, "\nDo it\n\nDue: Friday.\n");
        assert_eq!(due_in(&made.body).as_deref(), Some("Friday"));
        assert_eq!(due_in("\nno date here\n"), None);
    }
}
