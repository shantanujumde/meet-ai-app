//! The auto-sync queue with a fake `claude` (`test_support::FakeCli`) run
//! through the real harness and [`super::super::sync_in`]. No real CLI, no
//! tracker.

use std::fs;
use std::path::PathBuf;

use serde_json::json;
use store::ticket::Ticket;

use super::*;

const MEETING: &str = "2026-09-01-1430-standup";
const URL: &str = "https://linear.app/acme/issue/ENG-42";

/// A meetings folder with root tickets `ids` (approved from `MEETING`), and
/// a fake `claude` that answers every run with an issue (`ENG-42`) and
/// appends one line per run to `runs.log`.
struct World {
    root: tempfile::TempDir,
    bin: tempfile::TempDir,
    cli: test_support::FakeCli,
}

impl World {
    fn new(ids: &[&str]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(store::TICKETS_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(root.path().join(MEETING)).unwrap();
        for id in ids {
            let mut made = Ticket::new(id, &format!("Task {id}"), MEETING);
            made.body = "\nWhat to do and why.\n".to_owned();
            made.write(&dir.join(format!("{id}.md"))).unwrap();
        }
        let bin = tempfile::tempdir().unwrap();
        let cli = test_support::FakeCli::install(bin.path(), "claude");
        cli.set(
            "log_append",
            bin.path().join("runs.log").display().to_string(),
        );
        let world = Self { root, bin, cli };
        world.answer_with_an_issue();
        world
    }

    fn reply(&self, output: serde_json::Value) {
        let envelope = json!({
            "type": "result", "subtype": "success", "is_error": false,
            "structured_output": output,
        });
        self.cli.set("stdout", envelope.to_string());
    }

    fn answer_with_an_issue(&self) {
        self.reply(json!({ "external_id": "ENG-42", "external_url": URL, "refused_reason": null }));
    }

    /// What a run that could not reach the tracker answers (TUR-5).
    fn answer_with_nothing(&self) {
        self.reply(json!({ "external_id": null, "external_url": null, "refused_reason": null }));
    }

    /// How many times the fake CLI ran.
    fn runs(&self) -> usize {
        fs::read_to_string(self.bin.path().join("runs.log"))
            .map(|log| log.lines().count())
            .unwrap_or(0)
    }

    fn agent(&self) -> AgentConfig {
        AgentConfig {
            harness: HarnessChoice::ClaudeCode,
            binary_path: Some(self.cli.path().to_path_buf()),
            ..AgentConfig::default()
        }
    }

    /// Linear through `server`, chosen in Settings.
    fn setup(&self, server: &str) -> Option<Setup> {
        let tickets = TicketsConfig {
            tracker: "linear".to_owned(),
            tracker_mcp: server.to_owned(),
        };
        Setup::of(true, self.agent(), tickets)
    }

    fn sender(&self) -> Fake {
        Fake {
            root: self.root.path().to_path_buf(),
            runs: SyncRuns::default(),
            setup: Mutex::new(Ok(self.setup("claude.ai Linear"))),
            seen: Mutex::default(),
        }
    }

    fn ticket(&self, id: &str) -> Ticket {
        let path = self
            .root
            .path()
            .join(store::TICKETS_DIR)
            .join(format!("{id}.md"));
        Ticket::read(&path).unwrap()
    }
}

/// The app's [`Sender`] with the settings and the root swapped for the
/// test's, and every status it was given kept.
struct Fake {
    root: PathBuf,
    runs: SyncRuns,
    setup: Mutex<Result<Option<Setup>, UiError>>,
    seen: Mutex<Vec<TicketSyncStatus>>,
}

impl Fake {
    fn set_up(&self, setup: Result<Option<Setup>, UiError>) {
        *self.setup.lock().unwrap() = setup;
    }

    /// Every status sent since the last call, as `(ticket, state)`.
    fn seen(&self) -> Vec<(String, TicketSyncState)> {
        let seen = std::mem::take(&mut *self.seen.lock().unwrap());
        seen.into_iter().map(|s| (s.ticket_id, s.state)).collect()
    }

    fn last(&self) -> TicketSyncStatus {
        self.seen.lock().unwrap().last().cloned().unwrap()
    }
}

impl Sender for Fake {
    fn setup(&self) -> Result<Option<Setup>, UiError> {
        self.setup.lock().unwrap().clone()
    }

    fn send(&self, ticket_id: &str, setup: &Setup) -> Result<TicketSummary, UiError> {
        send_root(&self.runs, None, || Ok(self.root.clone()), ticket_id, setup)
    }

    fn emit(&self, status: &TicketSyncStatus) {
        self.seen.lock().unwrap().push(status.clone());
    }
}

/// Queue `ids` as approve or create does, then run the queue to its end if
/// this queue call started it.
fn trigger(auto: &AutoSync, fake: &Fake, ids: &[&str]) {
    if auto.queue(ids.iter().copied(), false, |s| fake.emit(s)) {
        auto.drain(fake);
    }
}

/// Retry on `id`.
fn retry(auto: &AutoSync, fake: &Fake, id: &str) {
    if auto.queue([id], true, |s| fake.emit(s)) {
        auto.drain(fake);
    }
}

fn sent(id: &str) -> Vec<(String, TicketSyncState)> {
    vec![
        (id.to_owned(), TicketSyncState::Queued),
        (id.to_owned(), TicketSyncState::Sending),
        (id.to_owned(), TicketSyncState::Sent),
    ]
}

#[test]
fn a_new_ticket_is_sent_once_and_its_issue_saved() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();

    trigger(&auto, &fake, &["TICK-0001", "TICK-0001"]);

    assert_eq!(world.runs(), 1);
    let last = fake.last();
    let ticket = last.ticket.expect("the sent ticket");
    assert_eq!(ticket.external_id.as_deref(), Some("ENG-42"));
    assert_eq!(ticket.meeting.as_deref(), Some(MEETING));
    assert_eq!(fake.seen(), sent("TICK-0001"));
    let back = world.ticket("TICK-0001");
    assert_eq!(back.synced_to().as_deref(), Some("linear"));
    assert_eq!(
        back.frontmatter.get_str("external_url").as_deref(),
        Some(URL)
    );
    assert!(auto.states().is_empty(), "{:?}", auto.states());

    // Queued again (a later Approve all, a settings save): already in the
    // tracker, so the agent never runs.
    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 1);
}

#[test]
fn tickets_are_sent_one_after_another_in_order() {
    let world = World::new(&["TICK-0001", "TICK-0002", "TICK-0003"]);
    let auto = AutoSync::default();
    let fake = world.sender();

    // A second trigger while the first is still queued joins its queue.
    assert!(auto.queue(["TICK-0002"], false, |s| fake.emit(s)));
    assert!(!auto.queue(["TICK-0001", "TICK-0003"], false, |s| fake.emit(s)));
    auto.drain(&fake);

    assert_eq!(world.runs(), 3);
    let sending: Vec<String> = fake
        .seen()
        .into_iter()
        .filter(|(_, state)| *state == TicketSyncState::Sending)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(sending, ["TICK-0002", "TICK-0001", "TICK-0003"]);
    // Drained, so the next trigger starts a new drain.
    assert!(auto.queue(["TICK-0001"], false, |_| {}));
}

#[test]
fn a_failure_is_not_tried_again_until_retry() {
    let world = World::new(&["TICK-0001"]);
    world.answer_with_nothing();
    let auto = AutoSync::default();
    let fake = world.sender();

    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 1);
    let failed = fake.last();
    assert_eq!(failed.state, TicketSyncState::Failed);
    let error = failed.error.expect("the error");
    assert_eq!(error.kind, "sync-unreachable", "{}", error.message);
    assert!(error.message.contains("press Retry"), "{}", error.message);
    let states = auto.states();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].state, TicketSyncState::Failed);
    assert!(world.ticket("TICK-0001").synced_to().is_none());
    fake.seen();

    // Queued again under the same settings: no run, and the error stays.
    world.answer_with_an_issue();
    for _ in 0..3 {
        trigger(&auto, &fake, &["TICK-0001"]);
    }
    assert_eq!(world.runs(), 1, "retried on its own");
    assert!(fake.seen().is_empty());
    assert_eq!(auto.states()[0].state, TicketSyncState::Failed);

    // Retry sends it.
    retry(&auto, &fake, "TICK-0001");
    assert_eq!(world.runs(), 2);
    assert_eq!(fake.seen(), sent("TICK-0001"));
    assert!(auto.states().is_empty());
    assert_eq!(
        world
            .ticket("TICK-0001")
            .frontmatter
            .get_str("external_id")
            .as_deref(),
        Some("ENG-42")
    );
}

#[test]
fn a_settings_change_tries_each_failure_once_more() {
    let world = World::new(&["TICK-0001", "TICK-0002"]);
    world.answer_with_nothing();
    let auto = AutoSync::default();
    let fake = world.sender();
    trigger(&auto, &fake, &["TICK-0001", "TICK-0002"]);
    assert_eq!(world.runs(), 2);

    // Settings, Tracker saved with another server: every unsent ticket once.
    fake.set_up(Ok(world.setup("linear")));
    trigger(&auto, &fake, &["TICK-0001", "TICK-0002"]);
    assert_eq!(world.runs(), 4);
    let error = fake.last().error.expect("failed again");
    assert!(error.message.contains("\"linear\""), "{}", error.message);

    // Saved again with the same values: nothing new to try.
    trigger(&auto, &fake, &["TICK-0001", "TICK-0002"]);
    assert_eq!(world.runs(), 4);
}

#[test]
fn with_no_tracker_set_up_nothing_is_sent_and_nothing_fails() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();
    fake.set_up(Ok(None));

    trigger(&auto, &fake, &["TICK-0001"]);

    assert_eq!(world.runs(), 0);
    let seen = fake.seen();
    assert_eq!(seen.last().map(|s| s.1), Some(TicketSyncState::NotSent));
    assert!(
        seen.iter().all(|s| s.1 != TicketSyncState::Failed),
        "{seen:?}"
    );
    assert!(auto.states().is_empty());

    // Set up later: the settings change sends it.
    fake.set_up(Ok(world.setup("claude.ai Linear")));
    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 1);
}

#[test]
fn settings_that_cannot_be_read_fail_the_ticket_without_a_run() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();
    fake.set_up(Err(UiError::app(
        "config-invalid",
        "config.jsonc is broken.",
    )));

    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 0);
    let failed = fake.last();
    assert_eq!(failed.state, TicketSyncState::Failed);
    assert_eq!(failed.error.map(|e| e.kind), Some("config-invalid"));

    // Not a failure under any settings: fixed, the next trigger sends it.
    fake.set_up(Ok(world.setup("claude.ai Linear")));
    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 1);
}

#[test]
fn a_ticket_gone_before_its_turn_is_just_not_sent() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();
    fs::remove_file(
        world
            .root
            .path()
            .join(store::TICKETS_DIR)
            .join("TICK-0001.md"),
    )
    .unwrap();

    trigger(&auto, &fake, &["TICK-0001"]);
    assert_eq!(world.runs(), 0);
    assert_eq!(fake.last().state, TicketSyncState::NotSent);
    assert!(auto.states().is_empty());
}

#[test]
fn a_signed_out_agent_fails_with_its_own_kind() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();
    let mut setup = world.setup("claude.ai Linear").unwrap();
    setup.agent.binary_path = Some(world.bin.path().join("no-such-claude"));
    fake.set_up(Ok(Some(setup)));

    trigger(&auto, &fake, &["TICK-0001"]);
    let error = fake.last().error.expect("failed");
    assert_eq!(error.kind, "agent-not-installed", "{}", error.message);
    assert!(
        error.message.ends_with("then press Retry."),
        "{}",
        error.message
    );
}

#[test]
fn retry_on_a_ticket_being_sent_is_not_a_second_send() {
    let mut queue = Queue::default();
    queue.add("TICK-0001", false);
    let key = World::new(&[]).setup("x").unwrap().key;
    assert_eq!(queue.next(Some(&key)), Next::Send("TICK-0001".to_owned()));
    assert!(queue.add("TICK-0001", true).is_none());
    assert!(queue.waiting.is_empty());
    assert!(queue.forced.is_empty());
}

#[test]
fn retry_on_a_failure_waiting_in_the_queue_shows_it_queued() {
    let mut queue = Queue::default();
    let key = World::new(&[]).setup("x").unwrap().key;
    queue.add("TICK-0001", false);
    queue.next(Some(&key));
    queue.finish(
        "TICK-0001",
        &key,
        Err(UiError::app("sync-unreachable", "no")),
    );
    assert!(
        queue.add("TICK-0001", false).is_none(),
        "still shown as failed"
    );
    let status = queue.add("TICK-0001", true).expect("queued");
    assert_eq!(status.state, TicketSyncState::Queued);
    assert_eq!(queue.waiting.len(), 1);
    assert_eq!(queue.next(Some(&key)), Next::Send("TICK-0001".to_owned()));
}

#[test]
fn only_a_chosen_tracker_and_agent_are_set_up() {
    let world = World::new(&[]);
    let tickets = TicketsConfig::default();
    assert!(Setup::of(false, world.agent(), tickets.clone()).is_none());
    let none = AgentConfig {
        harness: HarnessChoice::None,
        ..AgentConfig::default()
    };
    assert!(Setup::of(true, none, tickets.clone()).is_none());
    let setup = Setup::of(true, world.agent(), tickets).expect("set up");
    assert_eq!(setup.run.tickets.tracker_mcp, "claude.ai Linear");

    let a = world.setup("claude.ai Linear").unwrap().key;
    assert_eq!(a, setup.key);
    assert_ne!(a, world.setup("linear").unwrap().key);
    let mut codex = world.agent();
    codex.harness = HarnessChoice::Codex;
    assert_ne!(a, SettingsKey::of(&codex, &TicketsConfig::default()));
}

#[test]
fn a_settings_change_queues_only_root_tickets_not_in_the_tracker() {
    let row = |id: &str| TicketSummary {
        status: Some(store::ticket::Status::Open),
        ..crate::tickets::unreadable(id)
    };
    let mut synced = row("TICK-0002");
    synced.synced_to = Some("linear".to_owned());
    synced.external_url = Some(URL.to_owned());
    let mut suggestion = row("TICK-0003");
    suggestion.suggested = true;
    let mut dropped = row("TICK-0004");
    dropped.status = Some(store::ticket::Status::Dropped);
    let mut linked = row("TICK-0005");
    linked.external_url = Some(URL.to_owned());
    let rows = [
        row("TICK-0001"),
        synced,
        suggestion,
        dropped,
        linked,
        row("TICK-0006"),
    ];
    assert_eq!(unsent(&rows), ["TICK-0001", "TICK-0006"]);
}

#[test]
fn the_window_sees_states_by_their_names() {
    let status = TicketSyncStatus::new("TICK-0001", TicketSyncState::NotSent);
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        json,
        json!({ "ticketId": "TICK-0001", "state": "not_sent", "error": null, "ticket": null })
    );
}

/// A Sync press on the meeting page created the issue but could not save it
/// (a folder move held the root); the ticket was approved after. Sending it
/// on its own saves that issue and does not make a second one.
#[test]
fn an_issue_a_sync_press_kept_is_saved_not_made_again() {
    let world = World::new(&["TICK-0001"]);
    let auto = AutoSync::default();
    let fake = world.sender();
    let setup = world.setup("claude.ai Linear").unwrap();
    let gate = FolderGate::default();
    let moving = gate.begin_move().unwrap();
    let pressed = sync_in(
        &fake.runs,
        Some(&gate),
        || Ok(world.root.path().to_path_buf()),
        "TICK-0001",
        Some(MEETING),
        || ready(&setup.agent, setup.run.clone()),
    )
    .unwrap_err();
    drop(moving);
    assert_eq!(pressed.kind, "sync-not-saved", "{}", pressed.message);
    assert_eq!(world.runs(), 1);

    trigger(&auto, &fake, &["TICK-0001"]);

    assert_eq!(world.runs(), 1, "a second issue");
    assert_eq!(fake.last().state, TicketSyncState::Sent);
    assert_eq!(
        world
            .ticket("TICK-0001")
            .frontmatter
            .get_str("external_id")
            .as_deref(),
        Some("ENG-42")
    );
}
