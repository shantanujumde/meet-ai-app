//! TUR-32: finding meetings by title through the index, not the folders.

use std::fs;

use store::index::{Index, IndexedMeeting};

fn meeting(root: &std::path::Path, id: &str, title: &str, date: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("meeting.md"),
        format!("---\nid: {id}\ntitle: {title}\ndate: {date}\n---\n\n## Summary\n\nx\n"),
    )
    .unwrap();
}

#[test]
fn same_title_any_case_newest_first_and_nothing_else() {
    let root = tempfile::tempdir().unwrap();
    meeting(
        root.path(),
        "2026-09-01-1430-standup",
        "Platform Standup",
        "2026-09-01T14:30:00+05:30",
    );
    meeting(
        root.path(),
        "2026-09-08-1430-standup",
        "platform standup ",
        "2026-09-08T14:30:00+05:30",
    );
    meeting(
        root.path(),
        "2026-09-09-1000-retro",
        "Sprint Retro",
        "2026-09-09T10:00:00+05:30",
    );
    let index = Index::open(root.path()).unwrap();

    let found = index.meetings_titled("  PLATFORM standup").unwrap();
    let ids: Vec<&str> = found.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["2026-09-08-1430-standup", "2026-09-01-1430-standup"],
        "{found:?}"
    );
    assert_eq!(
        found[1],
        IndexedMeeting {
            id: "2026-09-01-1430-standup".into(),
            title: "Platform Standup".into(),
            date: Some("2026-09-01T14:30:00+05:30".into()),
        }
    );

    assert!(index.meetings_titled("Standup").unwrap().is_empty());
    assert!(index.meetings_titled("   ").unwrap().is_empty());
}

/// TUR-107: the app's own `meeting.md` writes are skipped by the watcher, so
/// the app refreshes the meeting in the index itself. A rename, the calendar
/// and the agent all go through this.
#[test]
fn a_refreshed_meeting_is_found_under_its_new_title() {
    let root = tempfile::tempdir().unwrap();
    let id = "2026-10-05-1000-meeting";
    meeting(root.path(), id, "Meeting", "2026-10-05T10:00:00+05:30");
    let mut index = Index::open(root.path()).unwrap();

    store::meeting_title::set_by_user(root.path(), id, "Platform Standup").unwrap();
    assert!(
        index
            .meetings_titled("Platform Standup")
            .unwrap()
            .is_empty(),
        "nothing has told the index yet"
    );

    index.refresh_meeting(root.path(), id).unwrap();
    let found = index.meetings_titled("Platform Standup").unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].id, id);
    assert!(index.meetings_titled("Meeting").unwrap().is_empty());
    let hits = index.search("platform").unwrap();
    assert!(hits.iter().any(|hit| hit.meeting_id == id), "{hits:?}");
}

#[test]
fn refreshing_a_deleted_meeting_drops_it_and_a_bad_id_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let id = "2026-10-05-1000-meeting";
    meeting(
        root.path(),
        id,
        "Platform Standup",
        "2026-10-05T10:00:00+05:30",
    );
    let mut index = Index::open(root.path()).unwrap();

    fs::remove_dir_all(root.path().join(id)).unwrap();
    index.refresh_meeting(root.path(), id).unwrap();
    assert!(
        index
            .meetings_titled("Platform Standup")
            .unwrap()
            .is_empty()
    );

    assert!(index.refresh_meeting(root.path(), "../escape").is_err());
}
