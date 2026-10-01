//! 4. Folder robustness.

use super::*;

#[test]
fn scan_returns_every_folder_of_a_hostile_tree_and_names_each_broken_file() {
    let scratch = Scratch::new("hostile-tree");
    let root = scratch.path();
    let bad_utf8: &[u8] = b"---\nid: m\n---\ncaf\xe9 \xff\xfe\n";
    let unicode_id = "2026-01-06-0900-会議-☕-é";

    let dir = |id: &str| {
        let d = root.join(id);
        fs::create_dir_all(&d).unwrap();
        d
    };

    let d = dir("2026-01-01-0900-meeting-md-is-a-folder");
    fs::create_dir_all(d.join(MEETING_FILE)).unwrap();

    let d = dir("2026-01-02-0900-tickets-is-a-file");
    fs::write(d.join(TICKETS_DIR), "not a folder").unwrap();

    let d = dir("2026-01-03-0900-empty-ticket");
    fs::create_dir_all(d.join(TICKETS_DIR)).unwrap();
    fs::write(d.join(TICKETS_DIR).join("TICK-0001.md"), "").unwrap();

    let d = dir("2026-01-04-0900-not-utf8");
    fs::create_dir_all(d.join(TICKETS_DIR)).unwrap();
    fs::write(d.join(MEETING_FILE), bad_utf8).unwrap();
    fs::write(d.join(TRANSCRIPT_FILE), b"[00:00:04] You: \xc3\x28\n").unwrap();
    fs::write(d.join(NOTES_FILE), b"caf\xe9 notes").unwrap();
    fs::write(d.join(TICKETS_DIR).join("TICK-0002.md"), bad_utf8).unwrap();

    let d = dir("2026-01-05-0900-notes-and-transcript-are-folders");
    fs::create_dir_all(d.join(NOTES_FILE)).unwrap();
    fs::create_dir_all(d.join(TRANSCRIPT_FILE)).unwrap();

    let d = dir(unicode_id);
    Meeting::new(unicode_id, "☕ 会議")
        .write(&d.join(MEETING_FILE))
        .unwrap();

    dir("2026-01-07-0900-empty");

    // Not meetings: a dot-folder and a plain file.
    fs::create_dir_all(root.join(".hidden").join(TICKETS_DIR)).unwrap();
    fs::write(root.join("2026-01-08-0900-a-file.md"), "---\n---\n").unwrap();

    let folders = folder::scan(root).expect("a hostile tree still scans");
    let ids: Vec<_> = folders.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "2026-01-07-0900-empty",
            unicode_id,
            "2026-01-05-0900-notes-and-transcript-are-folders",
            "2026-01-04-0900-not-utf8",
            "2026-01-03-0900-empty-ticket",
            "2026-01-02-0900-tickets-is-a-file",
            "2026-01-01-0900-meeting-md-is-a-folder",
        ]
    );
    let by_id = |id: &str| folders.iter().find(|f| f.id == id).unwrap();

    let f = by_id("2026-01-01-0900-meeting-md-is-a-folder");
    assert_eq!(labelled(f), pairs(&[("meeting.md", "Unreadable")]));
    assert!(f.meeting.is_none());

    let f = by_id("2026-01-02-0900-tickets-is-a-file");
    assert_eq!(labelled(f), pairs(&[("tickets", "Unreadable")]));
    assert!(f.tickets.is_empty());

    let f = by_id("2026-01-03-0900-empty-ticket");
    assert_eq!(
        labelled(f),
        pairs(&[
            ("tickets/TICK-0001.md", "NoFrontmatter"),
            ("tickets/TICK-0001.md", "MissingField(id)"),
            ("tickets/TICK-0001.md", "MissingField(title)"),
        ])
    );
    assert_eq!(f.tickets.len(), 1, "an empty ticket still loads");

    let f = by_id("2026-01-04-0900-not-utf8");
    assert_eq!(
        labelled(f),
        pairs(&[
            ("meeting.md", "Unreadable"),
            ("transcript.md", "Unreadable"),
            ("notes.md", "Unreadable"),
            ("tickets/TICK-0002.md", "Unreadable"),
        ])
    );
    // A lossy decode here would hand the notes pane U+FFFD, and its next
    // autosave would replace the real bytes for good.
    assert_eq!(f.notes, "");
    assert!(f.meeting.is_some() && f.transcript.is_some());
    assert_eq!(f.tickets.len(), 1);

    let f = by_id("2026-01-05-0900-notes-and-transcript-are-folders");
    assert_eq!(
        labelled(f),
        pairs(&[("transcript.md", "Unreadable"), ("notes.md", "Unreadable")])
    );

    let f = by_id(unicode_id);
    assert!(!f.needs_attention(), "{:?}", f.problems);
    assert_eq!(
        f.meeting.as_ref().unwrap().title().as_deref(),
        Some("☕ 会議")
    );

    let f = by_id("2026-01-07-0900-empty");
    assert!(!f.needs_attention(), "{:?}", f.problems);

    // The same tree, one folder at a time, agrees with the scan.
    for f in &folders {
        assert_eq!(&folder::load(&f.path).unwrap(), f, "{}", f.id);
    }
}
