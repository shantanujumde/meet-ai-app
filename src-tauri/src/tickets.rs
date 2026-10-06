//! Listing, hand-creating and approving tickets for the Tickets screen
//! (TUR-102, TUR-113).
//!
//! A thin adapter over `store::ticket` and `store::suggested`: `store` owns
//! the file format, this module turns it into the shape the webview renders
//! and picks the next id: the highest across the whole meetings root plus
//! one, under the same lock and rules a notes run numbers its tickets by
//! (`store::agent_notes`).
//!
//! The Tickets screen lists `<root>/tickets/`: the hand-made tickets, and the
//! suggested tasks the user approved from a meeting, labelled with that
//! meeting's title (SPEC A25). The suggestions themselves stay in their
//! meeting's own `tickets/` folder until approved or discarded.
//!
//! As everywhere else, a broken ticket file is a badge, not an error (SPEC §7).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use store::agent_notes;
use store::meeting::Meeting;
use store::suggested;
use store::ticket::{self, Ticket};
use store::watcher::SelfWrites;
use tauri::{AppHandle, Manager as _};

use crate::error::{UiError, on_blocking_pool};
use crate::meetings;
use crate::sync::{TICKET_MISSING, due_in, meeting_tasks_in};

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
    /// The tracker it was synced to (`linear`, `jira`, `github`); `None`
    /// until a Sync run created the issue (TUR-11).
    pub synced_to: Option<String>,
    /// The issue key the tracker gave it, e.g. `ENG-42`.
    pub external_id: Option<String>,
    /// The issue's web address.
    pub external_url: Option<String>,
    /// Still a suggestion in its meeting's own `tickets/` folder, waiting to
    /// be approved or discarded (SPEC A25). `false` for a ticket in Tickets.
    pub suggested: bool,
    /// Who it is for (the file's `assignee`).
    pub owner: Option<String>,
    /// When it is due, as said in the meeting (the body's `Due: X.` line).
    pub due: Option<String>,
    /// The title of the meeting it came from, for "From: <meeting>".
    pub meeting_title: Option<String>,
}

/// Every ticket in Tickets (`<root>/tickets/`), newest first.
pub fn list() -> Result<Vec<TicketSummary>, UiError> {
    list_in(&meetings::root()?)
}

/// Add a ticket by hand. No meeting is attached.
pub fn create(title: &str, body: &str) -> Result<TicketSummary, UiError> {
    create_in(&meetings::root()?, title, body)
}

// --- approving suggested tasks (TUR-113) -------------------------------------

/// Approve one of a meeting's suggested tasks: it moves to Tickets, keeping
/// its id and meeting. Returns it as Tickets now lists it.
#[tauri::command]
#[specta::specta]
pub async fn approve_task(
    app: AppHandle,
    meeting_id: String,
    ticket_id: String,
) -> Result<TicketSummary, UiError> {
    on_blocking_pool(move || {
        let self_writes = own_writes(&app);
        let approved = crate::folder_move::writing_in_root(&app, |root| {
            approve_in(root, &meeting_id, &ticket_id, &self_writes)
        });
        crate::search::meeting_written(&app, &meeting_id);
        approved
    })
    .await?
}

/// Approve every suggested task of a meeting that is still a suggestion.
/// Returns the meeting's tasks as they now are, as `meeting_tasks` does. A
/// task whose file cannot be moved stays a suggestion.
#[tauri::command]
#[specta::specta]
pub async fn approve_all_tasks(
    app: AppHandle,
    meeting_id: String,
) -> Result<Vec<TicketSummary>, UiError> {
    on_blocking_pool(move || {
        let self_writes = own_writes(&app);
        let tasks = crate::folder_move::writing_in_root(&app, |root| {
            approve_all_in(root, &meeting_id, &self_writes)
        });
        crate::search::meeting_written(&app, &meeting_id);
        tasks
    })
    .await?
}

/// Discard one of a meeting's suggested tasks: its file is deleted and its
/// number is never handed out again.
#[tauri::command]
#[specta::specta]
pub async fn discard_task(
    app: AppHandle,
    meeting_id: String,
    ticket_id: String,
) -> Result<(), UiError> {
    on_blocking_pool(move || {
        let self_writes = own_writes(&app);
        let discarded = crate::folder_move::writing_in_root(&app, |root| {
            discard_in(root, &meeting_id, &ticket_id, &self_writes)
        });
        crate::search::meeting_written(&app, &meeting_id);
        discarded
    })
    .await?
}

/// The update step for tickets from before suggestions: soon after launch, on
/// the blocking pool, move every meeting-folder ticket that was already synced
/// to Tickets ([`migrate_synced`]).
pub fn start_migration(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let self_writes = own_writes(&app);
        let moved =
            crate::folder_move::writing_in_root(&app, |root| migrate_synced(root, &self_writes));
        match moved {
            Ok(moved) => {
                for meeting_id in moved.iter().map(|m| m.meeting_id.as_str()) {
                    crate::search::meeting_written(&app, meeting_id);
                }
            }
            Err(error) => {
                tracing::warn!(message = %error.message, "could not move synced tickets to Tickets");
            }
        }
    });
}

/// Move every ticket in a meeting's folder that has `synced_to` set to
/// `<root>/tickets/`: it was synced, so it counts as approved. The rest stay
/// suggestions. Idempotent: a second run finds nothing to move. Each move is
/// logged.
pub(crate) fn migrate_synced(
    root: &Path,
    self_writes: &SelfWrites,
) -> Result<Vec<suggested::Moved>, UiError> {
    let moved = suggested::approve_synced(root, self_writes)?;
    for each in &moved {
        tracing::info!(
            meeting_id = %each.meeting_id,
            ticket_id = %each.ticket_id,
            "moved a synced ticket from its meeting to Tickets"
        );
    }
    Ok(moved)
}

fn approve_in(
    root: &Path,
    meeting_id: &str,
    ticket_id: &str,
    self_writes: &SelfWrites,
) -> Result<TicketSummary, UiError> {
    check_ticket_id(ticket_id)?;
    let path = suggested::approve(root, meeting_id, ticket_id, self_writes)
        .map_err(|error| suggestion_error(error, ticket_id))?;
    let mut summary = match Ticket::read(&path) {
        Ok(found) => summarize(ticket_id, &found),
        Err(_) => unreadable(ticket_id),
    };
    summary.meeting.get_or_insert_with(|| meeting_id.to_owned());
    name_meetings(root, std::slice::from_mut(&mut summary));
    Ok(summary)
}

fn approve_all_in(
    root: &Path,
    meeting_id: &str,
    self_writes: &SelfWrites,
) -> Result<Vec<TicketSummary>, UiError> {
    suggested::approve_all(root, meeting_id, self_writes)?;
    meeting_tasks_in(root, meeting_id)
}

fn discard_in(
    root: &Path,
    meeting_id: &str,
    ticket_id: &str,
    self_writes: &SelfWrites,
) -> Result<(), UiError> {
    check_ticket_id(ticket_id)?;
    suggested::discard(root, meeting_id, ticket_id, self_writes)
        .map_err(|error| suggestion_error(error, ticket_id))
}

/// A ticket id, never a path: it comes from the window.
fn check_ticket_id(ticket_id: &str) -> Result<(), UiError> {
    if ticket::parse_id(ticket_id).is_some() {
        return Ok(());
    }
    Err(UiError::app(
        "bad-ticket-id",
        format!("{ticket_id:?} is not a ticket id."),
    ))
}

/// `store::suggested`'s refusals in the window's words.
fn suggestion_error(error: store::Error, ticket_id: &str) -> UiError {
    match error {
        store::Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound => UiError::app(
            TICKET_MISSING,
            format!(
                "{ticket_id} is not a suggested task of this meeting any more. It may have been approved or discarded already."
            ),
        ),
        store::Error::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists => UiError::app(
            "ticket-already-in-tickets",
            format!(
                "Tickets already has a {ticket_id}, so this task stays here. Rename or remove the one in Tickets, then approve this one again."
            ),
        ),
        other => other.into(),
    }
}

/// The watcher's record of this process's own writes, so the files moved
/// here do not come back as outside changes (SPEC §4).
fn own_writes(app: &AppHandle) -> SelfWrites {
    app.try_state::<crate::watch::MeetingsWatch>()
        .map(|watch| watch.own_writes().clone())
        .unwrap_or_default()
}

// --- listing ---------------------------------------------------------------

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
        synced_to: ticket.synced_to(),
        external_id: ticket.frontmatter.get_str("external_id"),
        external_url: ticket.frontmatter.get_str("external_url"),
        suggested: false,
        owner: ticket.assignee().filter(|o| !o.trim().is_empty()),
        due: due_in(&ticket.body),
        meeting_title: None,
    }
}

/// [`summarize`] for the Sync commands, which list and update a meeting's
/// own tickets (TUR-11).
pub(crate) fn summary_of(stem: &str, ticket: &Ticket) -> TicketSummary {
    summarize(stem, ticket)
}

/// [`list`] under `root`, for the Sync commands.
pub(crate) fn list_under(root: &Path) -> Result<Vec<TicketSummary>, UiError> {
    list_in(root)
}

/// A ticket file that could not be read, still listed, flagged, so it is not
/// silently lost.
pub(crate) fn unreadable(stem: &str) -> TicketSummary {
    TicketSummary {
        id: stem.to_owned(),
        title: stem.to_owned(),
        status: None,
        meeting: None,
        body: String::new(),
        has_problems: true,
        synced_to: None,
        external_id: None,
        external_url: None,
        suggested: false,
        owner: None,
        due: None,
        meeting_title: None,
    }
}

/// Fill in each row's `meeting_title`: the meeting's `meeting.md` title, or
/// its folder name made readable. Each meeting is read once.
pub(crate) fn name_meetings(root: &Path, rows: &mut [TicketSummary]) {
    let mut titles: HashMap<String, Option<String>> = HashMap::new();
    for row in rows.iter_mut() {
        let Some(id) = row.meeting.clone() else {
            continue;
        };
        let title = titles
            .entry(id)
            .or_insert_with_key(|id| meeting_title(root, id))
            .clone();
        row.meeting_title = title;
    }
}

/// The title of meeting `meeting_id` for "From: <meeting>". `None` only for
/// an id that is not a folder name.
fn meeting_title(root: &Path, meeting_id: &str) -> Option<String> {
    let dir = store::folder::meeting_dir(root, meeting_id).ok()?;
    let written = Meeting::read(&dir.join(store::MEETING_FILE))
        .ok()
        .flatten()
        .and_then(|meeting| meeting.title())
        .filter(|title| !title.trim().is_empty());
    written.or_else(|| Some(folder_title(meeting_id)))
}

/// The folder's slug made readable, as a meeting with no `meeting.md` is
/// titled.
fn folder_title(meeting_id: &str) -> String {
    match store::folder_name::split_folder_name(meeting_id) {
        (_, _, Some(slug)) => store::folder_name::prettify_slug(slug),
        _ => meeting_id.to_owned(),
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
            Err(_) => unreadable(&stem),
        };
        rows.push((ticket::parse_id(&summary.id), summary));
    }
    // Newest (highest number) first; ids that are not numbers go last.
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let mut rows: Vec<TicketSummary> = rows.into_iter().map(|(_, s)| s).collect();
    name_meetings(root, &mut rows);
    Ok(rows)
}

fn create_in(root: &Path, title: &str, body: &str) -> Result<TicketSummary, UiError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(UiError::app(
            "ticket-title-empty",
            "Give the ticket a title before saving it.",
        ));
    }
    // Held until the file is on disk, as a notes run holds it while it writes
    // its tasks, so the two never hand out the same number (TUR-18).
    let _numbers = agent_notes::lock_meeting_writers();
    let dir = tickets_dir(root);
    fs::create_dir_all(&dir)?;

    // Across every meeting, and past any number a notes run keeps retired.
    let mut next = agent_notes::highest_ticket_number(root)?
        .max(agent_notes::highest_recorded_ticket_number(root)?)
        .saturating_add(1);

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

    fn temp_root(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("meet-ai-tickets-{name}-"))
            .tempdir()
            .expect("create temp root")
    }

    #[test]
    fn a_missing_tickets_folder_lists_nothing() {
        let root_dir = temp_root("empty");
        let root = root_dir.path();
        assert!(list_in(root).expect("list").is_empty());
    }

    #[test]
    fn create_then_list_round_trips() {
        let root_dir = temp_root("roundtrip");
        let root = root_dir.path();
        let made = create_in(root, "  Fix the thing  ", "Some notes").expect("create");
        assert_eq!(made.id, "TICK-0001");
        assert_eq!(made.title, "Fix the thing");
        assert_eq!(made.status.as_deref(), Some("open"));
        assert_eq!(made.meeting, None);
        assert_eq!(made.body, "Some notes\n");
        assert!(!made.has_problems);

        let listed = list_in(root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "TICK-0001");
        assert_eq!(listed[0].title, "Fix the thing");
        assert_eq!(listed[0].meeting, None);
        assert!(!listed[0].has_problems);
    }

    #[test]
    fn ids_increment_and_the_newest_is_first() {
        let root_dir = temp_root("ids");
        let root = root_dir.path();
        let a = create_in(root, "one", "").expect("a");
        let b = create_in(root, "two", "").expect("b");
        assert_eq!((a.id.as_str(), b.id.as_str()), ("TICK-0001", "TICK-0002"));
        let ids: Vec<_> = list_in(root)
            .expect("list")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["TICK-0002", "TICK-0001"]);
    }

    #[test]
    fn an_empty_title_is_rejected() {
        let root_dir = temp_root("blank");
        let root = root_dir.path();
        let error = create_in(root, "   ", "body").expect_err("rejected");
        assert_eq!(error.kind, "ticket-title-empty");
        assert!(list_in(root).expect("list").is_empty());
    }

    #[test]
    fn a_broken_ticket_is_listed_with_a_problem_flag() {
        let root_dir = temp_root("broken");
        let root = root_dir.path();
        let dir = tickets_dir(root);
        fs::create_dir_all(&dir).expect("dir");
        fs::write(
            dir.join("TICK-0007.md"),
            "---\ntitle: [unclosed\n---\nKeep.\n",
        )
        .expect("write");
        fs::write(dir.join("notes.txt"), "not a ticket").expect("write");
        let listed = list_in(root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "TICK-0007");
        assert_eq!(listed[0].title, "TICK-0007");
        assert!(listed[0].has_problems);
        // The next id still moves past a broken file.
        assert_eq!(create_in(root, "next", "").expect("create").id, "TICK-0008");
    }

    const MEETING: &str = "2026-09-01-1430-standup";

    fn meeting_folder(root: &Path) {
        fs::create_dir_all(root.join(MEETING)).expect("meeting folder");
    }

    fn notes_with(tasks: &[&str]) -> prompts::notes::Notes {
        prompts::notes::Notes {
            title: "Standup".to_owned(),
            summary: "Standup.".to_owned(),
            decisions: Vec::new(),
            open_questions: Vec::new(),
            tasks: tasks
                .iter()
                .map(|title| prompts::notes::Task {
                    title: (*title).to_owned(),
                    details: "From the call.".to_owned(),
                    owner: None,
                    due: None,
                    transcript_ref: "00:00:04".to_owned(),
                })
                .collect(),
        }
    }

    fn write_notes(root: &Path, tasks: &[&str]) -> Vec<String> {
        let analysis = agent_notes::Analysis {
            by: agent_notes::AnalyzedBy::ClaudeCode,
            model: "sonnet".to_owned(),
            at: "2026-09-01T15:32:00+05:30".to_owned(),
        };
        let self_writes = store::watcher::SelfWrites::default();
        agent_notes::write(root, MEETING, &notes_with(tasks), &analysis, &self_writes)
            .expect("notes written")
            .written
    }

    #[test]
    fn a_hand_made_ticket_and_a_notes_run_at_the_same_time_get_different_numbers() {
        for round in 0..10 {
            let root_dir = temp_root("race");
            let root = root_dir.path();
            meeting_folder(root);
            let start = std::sync::Barrier::new(2);
            let (agent, by_hand) = std::thread::scope(|s| {
                let agent = s.spawn(|| {
                    start.wait();
                    write_notes(root, &["Load test", "Move sessions to Redis"])
                });
                let by_hand = s.spawn(|| {
                    start.wait();
                    create_in(root, "By hand", "").expect("create")
                });
                (
                    agent.join().expect("agent thread"),
                    by_hand.join().expect("hand thread"),
                )
            });
            let mut ids = agent.clone();
            ids.push(by_hand.id.clone());
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), 3, "round {round}: {agent:?} and {}", by_hand.id);
        }
    }

    #[test]
    fn a_hand_made_ticket_waits_for_the_ticket_number_lock() {
        let root_dir = temp_root("lock");
        let root = root_dir.path();
        meeting_folder(root);
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            // Taken in here so a failed assert lets go of it before the join.
            let numbers = agent_notes::lock_meeting_writers();
            s.spawn(|| {
                let made = create_in(root, "By hand", "");
                done.send(made).ok();
            });
            assert!(
                finished
                    .recv_timeout(std::time::Duration::from_millis(300))
                    .is_err(),
                "created while another writer held the lock"
            );
            // What a notes run holding the lock lands before it lets go.
            let agent_dir = root.join(MEETING).join(store::TICKETS_DIR);
            fs::create_dir_all(&agent_dir).expect("agent tickets dir");
            Ticket::new("TICK-0001", "Load test", MEETING)
                .write(&agent_dir.join("TICK-0001.md"))
                .expect("agent ticket");
            drop(numbers);
            let made = finished.recv().expect("sent").expect("create");
            assert_eq!(made.id, "TICK-0002");
        });
    }

    #[test]
    fn a_number_a_notes_run_retired_is_not_handed_out_by_hand() {
        let root_dir = temp_root("retired");
        let root = root_dir.path();
        meeting_folder(root);
        assert_eq!(write_notes(root, &["Load test"]), ["TICK-0001"]);
        // The user deleted the agent's ticket; its number stays retired.
        fs::remove_file(
            root.join(MEETING)
                .join(store::TICKETS_DIR)
                .join("TICK-0001.md"),
        )
        .expect("delete");
        assert_eq!(
            create_in(root, "By hand", "").expect("create").id,
            "TICK-0002"
        );
    }

    // --- suggested tasks (TUR-113) ---------------------------------------------

    /// A meeting titled "Platform standup" whose notes run suggested the
    /// three tasks, TICK-0001 to TICK-0003.
    fn with_suggestions(name: &str) -> PathBuf {
        let root = temp_root(name);
        meeting_folder(&root);
        write_notes(&root, &["Load test", "Move sessions to Redis", "Runbook"]);
        let meeting_md = root.join(MEETING).join(store::MEETING_FILE);
        let mut meeting = Meeting::read(&meeting_md)
            .expect("read")
            .expect("meeting.md");
        meeting
            .frontmatter
            .set_str("title", Some("Platform standup"));
        meeting.write(&meeting_md).expect("retitle");
        root
    }

    fn suggestion(root: &Path, id: &str) -> PathBuf {
        root.join(MEETING)
            .join(store::TICKETS_DIR)
            .join(format!("{id}.md"))
    }

    #[test]
    fn approve_moves_a_suggestion_to_tickets_labelled_with_its_meeting() {
        let root = with_suggestions("approve");
        let writes = SelfWrites::default();
        assert!(
            list_in(&root).expect("list").is_empty(),
            "suggestions are not in Tickets"
        );

        let approved = approve_in(&root, MEETING, "TICK-0002", &writes).expect("approve");

        assert_eq!(approved.id, "TICK-0002");
        assert_eq!(approved.title, "Move sessions to Redis");
        assert_eq!(approved.meeting.as_deref(), Some(MEETING));
        assert_eq!(approved.meeting_title.as_deref(), Some("Platform standup"));
        assert_eq!(approved.status.as_deref(), Some("open"));
        assert!(!approved.suggested);
        assert!(
            approved.body.contains("From the call."),
            "{}",
            approved.body
        );
        assert!(!suggestion(&root, "TICK-0002").exists());

        let listed = list_in(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "TICK-0002");
        assert_eq!(listed[0].meeting_title.as_deref(), Some("Platform standup"));

        // Still the meeting's task, no longer a suggestion.
        let tasks = meeting_tasks_in(&root, MEETING).expect("tasks");
        let rows: Vec<(&str, bool)> = tasks.iter().map(|t| (t.id.as_str(), t.suggested)).collect();
        assert_eq!(
            rows,
            [
                ("TICK-0001", true),
                ("TICK-0002", false),
                ("TICK-0003", true)
            ]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn approving_twice_or_a_bad_id_says_so() {
        let root = with_suggestions("approve-twice");
        let writes = SelfWrites::default();
        approve_in(&root, MEETING, "TICK-0001", &writes).expect("approve");
        let again = approve_in(&root, MEETING, "TICK-0001", &writes).expect_err("gone");
        assert_eq!(again.kind, TICKET_MISSING);
        let bad = approve_in(&root, MEETING, "../TICK-0001", &writes).expect_err("bad");
        assert_eq!(bad.kind, "bad-ticket-id");
        assert_eq!(
            discard_in(&root, MEETING, "notes", &writes)
                .expect_err("bad")
                .kind,
            "bad-ticket-id"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn approve_never_overwrites_a_ticket_already_in_tickets() {
        let root = with_suggestions("approve-taken");
        fs::create_dir_all(tickets_dir(&root)).expect("dir");
        fs::write(tickets_dir(&root).join("TICK-0001.md"), "theirs").expect("write");
        let error =
            approve_in(&root, MEETING, "TICK-0001", &SelfWrites::default()).expect_err("taken");
        assert_eq!(error.kind, "ticket-already-in-tickets");
        assert!(suggestion(&root, "TICK-0001").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn approve_all_moves_the_rest_and_returns_the_meetings_tasks() {
        let root = with_suggestions("approve-all");
        let writes = SelfWrites::default();
        discard_in(&root, MEETING, "TICK-0002", &writes).expect("discard");

        let tasks = approve_all_in(&root, MEETING, &writes).expect("approve all");

        let rows: Vec<(&str, bool)> = tasks.iter().map(|t| (t.id.as_str(), t.suggested)).collect();
        assert_eq!(rows, [("TICK-0001", false), ("TICK-0003", false)]);
        let ids: Vec<String> = list_in(&root)
            .expect("list")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["TICK-0003", "TICK-0001"]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discarding_the_highest_task_keeps_its_number_from_a_hand_made_ticket() {
        let root = with_suggestions("discard");
        discard_in(&root, MEETING, "TICK-0003", &SelfWrites::default()).expect("discard");

        assert!(!suggestion(&root, "TICK-0003").exists());
        let ids: Vec<String> = meeting_tasks_in(&root, MEETING)
            .expect("tasks")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, ["TICK-0001", "TICK-0002"]);
        assert_eq!(
            create_in(&root, "By hand", "").expect("create").id,
            "TICK-0004"
        );
        // A notes re-run does not bring it back, nor take its number.
        let written = write_notes(&root, &["Load test", "Move sessions to Redis", "Runbook"]);
        assert!(!written.contains(&"TICK-0003".to_owned()), "{written:?}");
        assert!(!suggestion(&root, "TICK-0003").exists());

        let gone = discard_in(&root, MEETING, "TICK-0003", &SelfWrites::default())
            .expect_err("already gone");
        assert_eq!(gone.kind, TICKET_MISSING);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_update_step_moves_synced_tickets_once() {
        let root = with_suggestions("migrate");
        let path = suggestion(&root, "TICK-0002");
        let mut synced = Ticket::read(&path).expect("read");
        synced.frontmatter.set_str("synced_to", Some("linear"));
        synced.frontmatter.set_str("external_id", Some("ENG-42"));
        synced.write(&path).expect("write");

        let moved = migrate_synced(&root, &SelfWrites::default()).expect("migrate");

        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].ticket_id, "TICK-0002");
        assert_eq!(moved[0].meeting_id, MEETING);
        let listed = list_in(&root).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].external_id.as_deref(), Some("ENG-42"));
        assert_eq!(listed[0].meeting.as_deref(), Some(MEETING));
        assert!(suggestion(&root, "TICK-0001").exists());
        assert!(suggestion(&root, "TICK-0003").exists());

        let again = migrate_synced(&root, &SelfWrites::default()).expect("again");
        assert!(again.is_empty(), "{again:?}");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_ticket_lists_its_owner_and_due_date() {
        let root = temp_root("owner-due");
        meeting_folder(&root);
        let mut notes = notes_with(&["Ship it"]);
        notes.tasks[0].owner = Some("Priya".to_owned());
        notes.tasks[0].due = Some("Friday".to_owned());
        let analysis = agent_notes::Analysis {
            by: agent_notes::AnalyzedBy::ClaudeCode,
            model: "sonnet".to_owned(),
            at: "2026-09-01T15:32:00+05:30".to_owned(),
        };
        agent_notes::write(&root, MEETING, &notes, &analysis, &SelfWrites::default())
            .expect("notes");
        let tasks = meeting_tasks_in(&root, MEETING).expect("tasks");
        assert_eq!(tasks[0].owner.as_deref(), Some("Priya"));
        assert_eq!(tasks[0].due.as_deref(), Some("Friday"));
        assert!(tasks[0].suggested);
        // The title a fresh meeting.md gets: the folder name, made readable.
        assert_eq!(tasks[0].meeting_title.as_deref(), Some("Standup"));
        let made = create_in(&root, "By hand", "").expect("create");
        assert_eq!(
            (made.owner, made.due, made.meeting_title),
            (None, None, None)
        );
        fs::remove_dir_all(&root).ok();
    }
}
