use super::*;

const MEETING: &str = "2026-09-01-1430-search-sync";

const TRANSCRIPT: &str = "\
[00:00:04] Others: Morning everyone, let's look at the search work.
[00:00:05] Others: Priya will ship the search box by Friday.
[00:00:11] You: We're going with Postgres full-text search, not Elastic.
[00:09:00] Others: Unrelated chat much later.
";

fn temp_root(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("meet-ai-copy-prompt-{name}-"))
        .tempdir()
        .expect("create temp root")
}

/// A meeting folder with a transcript and, when `repo` is set, a
/// `meeting.md` that links it.
fn meeting(root: &Path, repo: Option<&str>) -> PathBuf {
    let dir = root.join(MEETING);
    fs::create_dir_all(dir.join(store::TICKETS_DIR)).expect("meeting dir");
    fs::write(dir.join(store::TRANSCRIPT_FILE), TRANSCRIPT).expect("transcript");
    if let Some(repo) = repo {
        fs::write(
            dir.join(store::MEETING_FILE),
            format!("---\nid: {MEETING}\ntitle: Search sync\nrepo: {repo}\n---\n"),
        )
        .expect("meeting.md");
    }
    dir
}

fn ticket(dir: &Path, id: &str, meeting: Option<&str>) {
    let mut made = Ticket::new(id, "Ship the search box", meeting.unwrap_or(""));
    if meeting.is_none() {
        made.frontmatter.set_str("meeting", None);
    }
    made.frontmatter.set_str("assignee", Some("Priya"));
    made.frontmatter.set_str("transcript_ref", Some("00:00:05"));
    made.body = "Use Postgres full-text search.\n".to_owned();
    fs::create_dir_all(dir).expect("tickets dir");
    made.write(&dir.join(format!("{id}.md"))).expect("ticket");
}

#[test]
fn start_work_fills_in_the_ticket_excerpt_and_repo() {
    let root_dir = temp_root("start");
    let root = root_dir.path();
    let dir = meeting(root, Some("~/apps/api"));
    ticket(&dir.join(store::TICKETS_DIR), "TICK-0007", Some(MEETING));

    let out = start_work_in(root, "TICK-0007", Some(MEETING), None).expect("render");
    assert!(out.contains("Ship the search box"), "{out}");
    assert!(out.contains("Use Postgres full-text search."), "{out}");
    assert!(out.contains("Priya"), "{out}");
    assert!(out.contains("~/apps/api"), "{out}");
    assert!(out.contains("Search sync"), "{out}");
    assert!(out.contains("[00:00:05] Others: Priya will ship the search box by Friday."));
    assert!(!out.contains("Unrelated chat much later"), "{out}");
    assert!(out.contains("TICK-0007.md"), "{out}");
}

#[test]
fn start_work_finds_the_meeting_from_the_ticket_and_uses_the_default_repo() {
    let root_dir = temp_root("shared");
    let root = root_dir.path();
    meeting(root, None);
    ticket(&root.join(store::TICKETS_DIR), "TICK-0002", Some(MEETING));

    let out = start_work_in(root, "TICK-0002", None, Some("~/apps/web".into())).expect("render");
    assert!(out.contains("~/apps/web"), "{out}");
    // No meeting.md: the title comes from the folder name, as in the list.
    assert!(out.contains("Search sync"), "{out}");
    assert!(out.contains("[00:00:05] Others: Priya will ship"), "{out}");
}

#[test]
fn start_work_for_a_hand_made_ticket_has_no_meeting() {
    let root_dir = temp_root("hand");
    let root = root_dir.path();
    ticket(&root.join(store::TICKETS_DIR), "TICK-0001", None);
    let out = start_work_in(root, "TICK-0001", None, None).expect("render");
    assert!(out.contains("Ship the search box"), "{out}");
    assert!(!out.contains("[00:00:05]"), "{out}");
}

#[test]
fn start_work_uses_the_users_own_template() {
    let root_dir = temp_root("template");
    let root = root_dir.path();
    ticket(&root.join(store::TICKETS_DIR), "TICK-0001", None);
    let prompts_dir = prompts::wrap_up::prompts_dir(root);
    fs::create_dir_all(&prompts_dir).expect("prompts dir");
    fs::write(prompts_dir.join("start-work.md"), "Do {{ ticket_id }}").expect("template");
    let out = start_work_in(root, "TICK-0001", None, None).expect("render");
    assert_eq!(out, "Do TICK-0001");

    fs::write(prompts_dir.join("start-work.md"), "{% if %}").expect("template");
    let error = start_work_in(root, "TICK-0001", None, None).expect_err("broken");
    assert_eq!(error.kind, "prompt-template");
}

#[test]
fn start_work_refuses_a_missing_ticket_and_a_path() {
    let root_dir = temp_root("missing");
    let root = root_dir.path();
    for id in ["TICK-0404", "../secret", "", ".hidden"] {
        let error = start_work_in(root, id, None, None).expect_err("refused");
        assert_eq!(error.kind, "ticket-not-found", "{id}");
    }
}

#[test]
fn wrap_up_is_the_clipboard_prompt_with_the_next_free_ticket_number() {
    let root_dir = temp_root("wrap");
    let root = root_dir.path();
    let dir = meeting(root, Some("~/apps/api"));
    ticket(&dir.join(store::TICKETS_DIR), "TICK-0003", Some(MEETING));
    ticket(&root.join(store::TICKETS_DIR), "TICK-0009", None);
    store::notes::write(&dir, "- check index size\n").expect("notes");

    let out = wrap_up_in(root, MEETING).expect("render");
    assert!(out.contains("analyzed_by: clipboard"), "{out}");
    assert!(out.contains("TICK-0010.md"), "{out}");
    assert!(out.contains(&dir.join(store::MEETING_FILE).display().to_string()));
    assert!(out.contains("<title>Search sync</title>"), "{out}");
    assert!(out.contains("check index size"), "{out}");
    assert!(out.contains("Unrelated chat much later"), "{out}");
}

#[test]
fn wrap_up_without_meeting_md_dates_from_the_folder_name() {
    let root_dir = temp_root("wrap-date");
    let root = root_dir.path();
    meeting(root, None);
    let out = wrap_up_in(root, MEETING).expect("render");
    assert!(out.contains("Date: 2026-09-01 14:30"), "{out}");
    assert!(out.contains("TICK-0001.md"), "{out}");
}

#[test]
fn wrap_up_refuses_an_empty_or_missing_meeting() {
    let root_dir = temp_root("wrap-empty");
    let root = root_dir.path();
    let dir = meeting(root, None);
    fs::write(dir.join(store::TRANSCRIPT_FILE), "\n").expect("empty");
    assert_eq!(
        wrap_up_in(root, MEETING).expect_err("empty").kind,
        "transcript-empty"
    );
    assert_eq!(
        wrap_up_in(root, "2026-01-01-0000-nope")
            .expect_err("missing")
            .kind,
        "meeting-not-found"
    );
    assert!(wrap_up_in(root, "../elsewhere").is_err());
}
