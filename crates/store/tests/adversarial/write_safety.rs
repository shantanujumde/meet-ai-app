//! 5. Write safety.

use super::*;

const BROKEN_YAML: &[&str] = &[
    "---\nid: m1\ntitle: [unclosed\n---\n## Summary\nkeep\n",
    "---\nid: m1\nid: m2\n---\n",
    "---\n- a\n- b\n---\n",
    "---\njust a scalar\n---\n",
    "---\na: 1\n...\nb: 2\n---\n",
    "---\na: 1\n--- \nb: 2\n---\n",
    "---\nkey: \"open quote\n---\n",
    "---\nkey: 'open\n---\n",
    "---\na: *undefined\n---\n",
    "\u{feff}---\r\nid: m1\r\ntitle: {a: 1\r\n---\r\nbody\r\n",
    "---\nid: m1\n  title: bad indent\n---\n",
    "---\n{a: 1}: [\n---\n",
];

/// Invalid UTF-8 of every kind: a stray continuation byte, a truncated
/// sequence at the end, an overlong encoding, an encoded surrogate, 0xFF.
const BAD_BYTES: &[&[u8]] = &[
    b"---\nid: m1\ntitle: T\n---\n\x80",
    b"---\nid: m1\ntitle: T\n---\nend \xe2\x82",
    b"---\nid: m1\ntitle: T\n---\n\xc0\xaf",
    b"---\nid: m1\ntitle: T\n---\n\xed\xa0\x80",
    b"\xff\xfe---\n",
    b"---\nid: \xf0\x9f\x8e\n---\n",
];

#[test]
fn a_file_that_loaded_broken_is_never_overwritten() {
    let scratch = Scratch::new("refuse");
    let dir = scratch.path();
    let meeting_path = dir.join(MEETING_FILE);
    let ticket_path = dir.join("TICK-0001.md");
    let mut inputs: Vec<Vec<u8>> = BROKEN_YAML.iter().map(|s| s.as_bytes().to_vec()).collect();
    inputs.extend(BAD_BYTES.iter().map(|b| b.to_vec()));

    for bytes in &inputs {
        let shown = String::from_utf8_lossy(bytes);
        fs::write(&meeting_path, bytes).unwrap();
        fs::write(&ticket_path, bytes).unwrap();

        let meeting = Meeting::read(&meeting_path).unwrap().unwrap();
        assert!(
            is_refused(&meeting.problems),
            "{shown:?}: {:?}",
            meeting.problems
        );
        assert!(meeting.render().is_err(), "{shown:?}");
        assert!(
            matches!(
                meeting.write(&meeting_path),
                Err(Error::Frontmatter { .. } | Error::Io(_))
            ),
            "{shown:?}"
        );
        // Even after a caller edits it, it must stay refused.
        let mut edited = meeting.clone();
        edited.set_section("Summary", "new text");
        edited.frontmatter.set_str("id", Some("m1"));
        assert!(edited.write(&meeting_path).is_err(), "{shown:?}");

        let ticket = Ticket::read(&ticket_path).unwrap();
        assert!(
            is_refused(&ticket.problems),
            "{shown:?}: {:?}",
            ticket.problems
        );
        assert!(ticket.render().is_err(), "{shown:?}");
        let mut edited = ticket.clone();
        edited.set_status(store::ticket::Status::Done);
        assert!(edited.write(&ticket_path).is_err(), "{shown:?}");

        assert_eq!(&fs::read(&meeting_path).unwrap(), bytes, "{shown:?}");
        assert_eq!(&fs::read(&ticket_path).unwrap(), bytes, "{shown:?}");
        assert_eq!(dotfiles(dir), Vec::<String>::new(), "{shown:?}");
    }
}

#[test]
fn notes_with_everything_nasty_in_them_round_trip_exactly() {
    let scratch = Scratch::new("notes");
    let dir = scratch.path();
    let mut rng = Rng::new(0x5eed_000a);
    let mut bodies: Vec<String> = vec![
        String::new(),
        "\u{feff}".into(),
        "\0".into(),
        "\r".into(),
        "\r\n".into(),
        "no newline".into(),
        "---\nid: not frontmatter\n---\n## Summary\n".into(),
        PIECES.concat(),
        LOOK_ALIKES.concat(),
    ];
    bodies.extend((0..200).map(|_| nasty_text(&mut rng, 30)));
    for body in &bodies {
        notes::write(dir, body).unwrap();
        assert_eq!(&notes::read(dir).unwrap(), body);
        assert_eq!(fs::read(dir.join(NOTES_FILE)).unwrap(), body.as_bytes());
    }
    assert_eq!(dotfiles(dir), Vec::<String>::new());
}

#[test]
fn notes_that_are_not_utf8_are_never_overwritten() {
    let scratch = Scratch::new("notes-bad-bytes");
    let dir = scratch.path();
    let path = dir.join(NOTES_FILE);
    let invalid_data = |result: Result<(), Error>| matches!(result, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::InvalidData);
    for bytes in BAD_BYTES {
        let shown = String::from_utf8_lossy(bytes);
        fs::write(&path, bytes).unwrap();
        assert!(invalid_data(notes::read(dir).map(drop)), "{shown:?}");
        assert!(invalid_data(notes::write(dir, "autosave")), "{shown:?}");
        assert!(invalid_data(notes::write(dir, "")), "{shown:?}");
        assert_eq!(fs::read(&path).unwrap(), *bytes, "{shown:?}");
        assert_eq!(dotfiles(dir), Vec::<String>::new(), "{shown:?}");
    }
}

#[test]
fn successful_writes_never_leave_a_temp_file_behind() {
    let scratch = Scratch::new("no-temp");
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_000b);
    let dir = scratch.path().join("2026-09-01-1430-standup");
    let tickets = dir.join(TICKETS_DIR);
    let mut writes = 0;
    for i in 0..300 {
        let raw = generated(&mut rng, &seeds);
        let meeting = Meeting::parse(&raw);
        if !is_refused(&meeting.problems) {
            let path = dir.join(MEETING_FILE);
            meeting.write(&path).unwrap();
            let back = Meeting::read(&path).unwrap().unwrap();
            assert_eq!(back.sections, meeting.sections);
            writes += 1;
        }
        let ticket = Ticket::parse(&raw);
        if !is_refused(&ticket.problems) {
            let path = tickets.join(format!("TICK-{:04}.md", i % 7 + 1));
            ticket.write(&path).unwrap();
            assert_eq!(Ticket::read(&path).unwrap().body, ticket.body);
            writes += 1;
        }
        notes::write(&dir, &raw).unwrap();
    }
    assert!(writes > 200, "only {writes} writes");
    assert_eq!(dotfiles(&dir), Vec::<String>::new());
    assert_eq!(dotfiles(&tickets), Vec::<String>::new());
    assert_eq!(folder::load(&dir).unwrap().tickets.len(), 7);
}

#[test]
fn a_failed_write_leaves_no_temp_file_behind() {
    // `write_atomic` writes a dotfile temp (`.notes.md.tmp.<pid>.<n>`), then
    // renames it over the target. When the rename fails (here: the target is
    // a non-empty folder) the temp file must be cleaned up, not left behind.
    let scratch = Scratch::new("failed-write");
    let dir = scratch.path();
    fs::create_dir_all(dir.join(NOTES_FILE).join("child")).unwrap();
    // `notes::write` now refuses before it gets this far (the folder cannot
    // be read as notes), so the rename failure is driven directly.
    assert!(notes::write(dir, "draft").is_err());
    assert!(store::write_atomic(&dir.join(NOTES_FILE), "draft").is_err());
    fs::create_dir_all(dir.join(MEETING_FILE).join("child")).unwrap();
    assert!(
        Meeting::new("m1", "T")
            .write(&dir.join(MEETING_FILE))
            .is_err()
    );
    assert_eq!(dotfiles(dir), Vec::<String>::new());
}
