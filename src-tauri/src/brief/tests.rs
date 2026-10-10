use super::*;

use std::fs;
use std::process::Command;

use store::index::Index;
use store::ticket::Ticket;

/// The day the brief is opened in these tests.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 15).expect("valid date")
}

/// A meeting folder with `meeting.md` and, for each `(id, status)`, a ticket.
fn meeting(
    root: &Path,
    id: &str,
    title: &str,
    date: &str,
    repo: Option<&str>,
    tickets: &[(&str, Status)],
) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).expect("meeting dir");
    let repo_line = repo.map(|r| format!("repo: {r}\n")).unwrap_or_default();
    fs::write(
        dir.join(store::MEETING_FILE),
        format!(
            "---\nid: {id}\ntitle: {title}\ndate: {date}\n{repo_line}---\n\n\
             ## Summary\n\nSummary of {id}.\n\n\
             ## Decisions\n\n- Decided in {id}.\n\n\
             ## Action Items\n\n## Open Questions\n"
        ),
    )
    .expect("meeting.md");
    let tickets_dir = dir.join(store::TICKETS_DIR);
    fs::create_dir_all(&tickets_dir).expect("tickets dir");
    for (ticket_id, status) in tickets {
        let mut ticket = Ticket::new(ticket_id, &format!("Task {ticket_id}"), id);
        ticket.set_status(*status);
        ticket
            .write(&tickets_dir.join(format!("{ticket_id}.md")))
            .expect("ticket");
    }
}

/// Run `git` in `repo`, committing as a fixed test user at `date`.
fn git(repo: &Path, date: &str, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn commit(repo: &Path, date: &str, message: &str) {
    git(
        repo,
        date,
        &["commit", "-q", "--allow-empty", "-m", message],
    );
}

/// A repo with one commit before 2026-09-08, two after it and a merge after
/// it.
fn temp_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("temp repo");
    let path = repo.path();
    git(
        path,
        "2026-08-01T10:00:00+05:30",
        &["init", "-q", "-b", "main"],
    );
    commit(path, "2026-08-01T10:00:00+05:30", "Old work");
    commit(path, "2026-09-09T10:00:00+05:30", "Add Redis sessions");
    git(
        path,
        "2026-09-10T10:00:00+05:30",
        &["checkout", "-q", "-b", "side"],
    );
    commit(path, "2026-09-10T10:00:00+05:30", "Side fix");
    git(
        path,
        "2026-09-10T11:00:00+05:30",
        &["checkout", "-q", "main"],
    );
    git(
        path,
        "2026-09-11T10:00:00+05:30",
        &["merge", "-q", "--no-ff", "side", "-m", "Merge side"],
    );
    commit(path, "2026-09-12T10:00:00+05:30", "Load test login");
    repo
}

fn brief(root: &Path, title: &str, default_repo: Option<String>) -> MeetingBrief {
    let matches = Index::open(root)
        .expect("index")
        .meetings_titled(title)
        .expect("titled");
    brief_from(root, title, &matches, today(), default_repo).expect("brief")
}

fn subjects(brief: &MeetingBrief) -> Vec<String> {
    brief
        .commits
        .as_ref()
        .map(|c| c.commits.iter().map(|c| c.subject.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn title_match_shows_last_time_and_commits_since() {
    let root = tempfile::tempdir().expect("root");
    let repo = temp_repo();
    let repo_path = repo.path().to_string_lossy().into_owned();
    meeting(
        root.path(),
        "2026-09-01-1430-standup",
        "Platform Standup",
        "2026-09-01T14:30:00+05:30",
        None,
        &[],
    );
    meeting(
        root.path(),
        "2026-09-08-1430-standup",
        "platform standup",
        "2026-09-08T14:30:00+05:30",
        Some(&repo_path),
        &[
            ("TICK-0001", Status::Open),
            ("TICK-0002", Status::InProgress),
            ("TICK-0003", Status::Done),
            ("TICK-0004", Status::Dropped),
        ],
    );
    // Today's own meeting is not "last time".
    meeting(
        root.path(),
        "2026-09-15-1430-standup",
        "Platform Standup",
        "2026-09-15T14:30:00+05:30",
        None,
        &[],
    );
    meeting(
        root.path(),
        "2026-09-10-1000-retro",
        "Sprint Retro",
        "2026-09-10T10:00:00+05:30",
        None,
        &[],
    );

    let brief = brief(root.path(), " Platform Standup ", None);

    let previous = brief.previous.as_ref().expect("a previous meeting");
    assert_eq!(previous.id, "2026-09-08-1430-standup");
    assert_eq!(previous.date.as_deref(), Some("2026-09-08T14:30:00+05:30"));
    assert_eq!(
        previous.summary.as_deref(),
        Some("Summary of 2026-09-08-1430-standup.")
    );
    assert_eq!(
        previous.decisions.as_deref(),
        Some("- Decided in 2026-09-08-1430-standup.")
    );
    let open: Vec<(&str, Option<Status>)> = previous
        .open_tickets
        .iter()
        .map(|t| (t.id.as_str(), t.status))
        .collect();
    assert_eq!(
        open,
        [
            ("TICK-0001", Some(Status::Open)),
            ("TICK-0002", Some(Status::InProgress))
        ]
    );
    assert_eq!(previous.open_tickets[0].title, "Task TICK-0001");

    let commits = brief.commits.as_ref().expect("commits");
    assert_eq!(commits.repo, repo_path);
    assert_eq!(commits.since, "2026-09-08T14:30:00+05:30");
    assert_eq!(
        subjects(&brief),
        ["Load test login", "Side fix", "Add Redis sessions"],
        "newest first, no merge, nothing before last time"
    );
    assert!(commits.commits.iter().all(|c| !c.hash.is_empty()));
}

#[test]
fn no_previous_meeting_leaves_both_parts_out() {
    let root = tempfile::tempdir().expect("root");
    let repo = temp_repo();
    meeting(
        root.path(),
        "2026-09-15-0900-kickoff",
        "Kickoff",
        "2026-09-15T09:00:00+05:30",
        None,
        &[],
    );
    let default_repo = Some(repo.path().to_string_lossy().into_owned());

    let only_today = brief(root.path(), "Kickoff", default_repo.clone());
    assert_eq!(only_today.title, "Kickoff");
    assert_eq!(only_today.previous, None);
    assert_eq!(only_today.commits, None);

    let never = brief(root.path(), "Board review", default_repo);
    assert_eq!(never.previous, None);
    assert_eq!(never.commits, None);
}

#[test]
fn no_repo_leaves_the_commits_out_quietly() {
    let root = tempfile::tempdir().expect("root");
    let plain = tempfile::tempdir().expect("plain folder");
    meeting(
        root.path(),
        "2026-09-08-1430-standup",
        "Standup",
        "2026-09-08T14:30:00+05:30",
        None,
        &[("TICK-0001", Status::Open)],
    );

    let none_set = brief(root.path(), "Standup", None);
    assert!(none_set.previous.is_some());
    assert_eq!(none_set.commits, None);

    let missing = brief(
        root.path(),
        "Standup",
        Some(plain.path().join("gone").to_string_lossy().into_owned()),
    );
    assert!(missing.previous.is_some());
    assert_eq!(missing.commits, None);

    let not_git = brief(
        root.path(),
        "Standup",
        Some(plain.path().to_string_lossy().into_owned()),
    );
    assert!(not_git.previous.is_some());
    assert_eq!(not_git.commits, None);
}

#[test]
fn default_repo_is_used_when_the_meeting_names_none() {
    let root = tempfile::tempdir().expect("root");
    let repo = temp_repo();
    meeting(
        root.path(),
        "2026-09-08-1430-standup",
        "Standup",
        "2026-09-08T14:30:00+05:30",
        None,
        &[],
    );
    let default_repo = repo.path().to_string_lossy().into_owned();

    let brief = brief(root.path(), "Standup", Some(default_repo.clone()));
    assert_eq!(
        brief.commits.as_ref().map(|c| c.repo.as_str()),
        Some(default_repo.as_str())
    );
    assert_eq!(subjects(&brief).len(), 3);
}

#[test]
fn commits_are_capped() {
    let repo = temp_repo();
    let commits =
        git::recent_commits(repo.path(), "2026-09-08T14:30:00+05:30", 2).expect("a git repo");
    let subjects: Vec<&str> = commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["Load test login", "Side fix"]);
}

/// TUR-168: a git that hangs (a repo on a stalled network share) is stopped
/// at the time limit, and the brief leaves the commits out.
#[test]
fn a_git_that_hangs_is_stopped_and_the_commits_left_out() {
    let bin = tempfile::tempdir().expect("temp dir");
    let slow_git = test_support::FakeCli::install(bin.path(), "git");
    slow_git
        .set("sleep", "30")
        .set("stdout", "abc1234 Never seen\n");
    let repo = tempfile::tempdir().expect("temp dir");

    let started = std::time::Instant::now();
    let commits = git::recent_commits_with(
        slow_git.path().as_os_str(),
        repo.path(),
        "2026-09-08T14:30:00+05:30",
        5,
        std::time::Duration::from_millis(300),
    );
    assert!(commits.is_none());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "git was not stopped: {:?}",
        started.elapsed()
    );

    // The same fake, quick, is read as git's output.
    slow_git.set("sleep", "0");
    let quick = git::recent_commits_with(
        slow_git.path().as_os_str(),
        repo.path(),
        "2026-09-08T14:30:00+05:30",
        5,
        std::time::Duration::from_secs(10),
    )
    .expect("a quick git");
    assert_eq!(quick.len(), 1);
    assert_eq!(quick[0].subject, "Never seen");
}

#[test]
fn the_day_falls_back_to_the_folder_name() {
    let undated = IndexedMeeting {
        id: "2026-09-08-1430-standup".into(),
        title: "Standup".into(),
        date: None,
    };
    assert_eq!(day_of(&undated), NaiveDate::from_ymd_opt(2026, 9, 8));
    let odd = IndexedMeeting {
        id: "standup".into(),
        title: "Standup".into(),
        date: Some("last week".into()),
    };
    assert_eq!(day_of(&odd), None);
}

#[test]
fn home_is_expanded_only_for_a_leading_tilde() {
    let home = dirs::home_dir().expect("home");
    assert_eq!(expand_home("~/apps/api"), home.join("apps/api"));
    assert_eq!(expand_home("~"), home);
    assert_eq!(expand_home("/srv/api"), PathBuf::from("/srv/api"));
    assert_eq!(expand_home("~other/api"), PathBuf::from("~other/api"));
}
