//! TUR-149: a meetings-folder move that fails at any step leaves every
//! meeting in exactly one folder, the one the app points at.
//!
//! The pointer is a closure here, never the real `root.json`: these tests must
//! not touch the developer's own config folder.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::move_tree::{JUNK, MoveFs, RealFs, move_root, point_at};
use super::{POINTER_UNREADABLE, RootPointer, paths, pointer_body, read_pointer, write_pointer_at};
use crate::error::UiError;
use crate::meetings::platform;

const MEETINGS: [&str; 4] = [
    "2026-09-01-0900-standup",
    "2026-09-02-0900-retro",
    "2026-09-03-0900-planning",
    "2026-09-04-0900-review",
];

/// An old meetings folder with four meetings, each with a transcript and an
/// audio folder.
fn old_folder(parent: &Path) -> PathBuf {
    let root = parent.join("old");
    for meeting in MEETINGS {
        let dir = root.join(meeting);
        fs::create_dir_all(dir.join("audio")).unwrap();
        fs::write(
            dir.join("transcript.md"),
            format!("[00:00:04] You: {meeting}\n"),
        )
        .unwrap();
        fs::write(dir.join("audio").join("mic.wav"), vec![7_u8; 4_096]).unwrap();
    }
    root
}

/// Every meeting is in `root`, whole.
fn assert_all_meetings_in(root: &Path) {
    for meeting in MEETINGS {
        let dir = root.join(meeting);
        assert_eq!(
            fs::read_to_string(dir.join("transcript.md")).unwrap(),
            format!("[00:00:04] You: {meeting}\n"),
            "{meeting} in {}",
            root.display()
        );
        assert_eq!(
            fs::read(dir.join("audio").join("mic.wav")).unwrap().len(),
            4_096
        );
    }
}

/// No meeting is in `root`, not even part of one.
fn assert_no_meetings_in(root: &Path) {
    for meeting in MEETINGS {
        assert!(
            fs::symlink_metadata(root.join(meeting)).is_err(),
            "{meeting} must not be in {}",
            root.display()
        );
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A pointer that remembers what it was asked to save, and can refuse.
struct Pointer {
    saved: RefCell<Option<PathBuf>>,
    fails: bool,
}

impl Pointer {
    fn ok() -> Self {
        Self {
            saved: RefCell::new(None),
            fails: false,
        }
    }

    fn failing() -> Self {
        Self {
            fails: true,
            ..Self::ok()
        }
    }

    fn save(&self, root: &Path) -> Result<(), UiError> {
        if self.fails {
            return Err(UiError::app("io", "No space left on device"));
        }
        *self.saved.borrow_mut() = Some(root.to_path_buf());
        Ok(())
    }

    fn saved(&self) -> Option<PathBuf> {
        self.saved.borrow().clone()
    }
}

/// The real filesystem, except: `rename` can act like a different volume, and
/// the copy of the `fail_after`-th file dies halfway (a full disk, a file
/// another program holds).
#[derive(Default)]
struct Faulty {
    cross_volume: bool,
    fail_after: Option<usize>,
    /// A copy that reports success but wrote less than the file has.
    short_copy: bool,
    /// Deleting the originals fails (a file another program holds).
    keep_originals: bool,
    /// Something lands in this folder after the copy, while the originals are
    /// being deleted (iCloud finishing a download).
    arrives_in: Option<PathBuf>,
    copies: Cell<usize>,
}

impl MoveFs for Faulty {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.cross_volume {
            return Err(io::Error::other("Cross-device link"));
        }
        fs::rename(from, to)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<u64> {
        let n = self.copies.get() + 1;
        self.copies.set(n);
        if self.fail_after == Some(n) {
            fs::write(to, b"half").unwrap();
            return Err(io::Error::other("No space left on device"));
        }
        if self.short_copy && from.file_name() == Some(OsStr::new("mic.wav")) {
            fs::write(to, b"short").unwrap();
            return Ok(4_096);
        }
        fs::copy(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = &self.arrives_in {
            fs::write(dir.join("2026-09-05-0900-downloading"), "still syncing").unwrap();
        }
        if self.keep_originals {
            return Err(io::Error::other("The file is in use"));
        }
        fs::remove_file(path)
    }
}

fn run(old: &Path, new: &Path, fs_ops: &dyn MoveFs, pointer: &Pointer) -> Result<(), UiError> {
    move_root(old, new, fs_ops, &|root| pointer.save(root))
}

// --- a good move ----------------------------------------------------------

#[test]
fn moving_into_a_folder_that_does_not_exist_yet_takes_everything_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    let pointer = Pointer::ok();

    run(&old, &new, &RealFs, &pointer).expect("the move must succeed");

    assert!(!old.exists(), "the old folder must not linger");
    assert_all_meetings_in(&new);
    assert_eq!(pointer.saved(), Some(new));
}

#[test]
fn a_cross_volume_move_copies_everything_then_deletes_the_old_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    fs::write(old.join(JUNK[0]), "finder").unwrap();
    let new = tmp.path().join("other-volume").join("Meetings");
    let pointer = Pointer::ok();
    let fs_ops = Faulty {
        cross_volume: true,
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &pointer).expect("the copy path must succeed");

    assert_all_meetings_in(&new);
    assert!(!old.exists(), "a folder holding only junk is removed");
    assert_eq!(pointer.saved(), Some(new));
}

#[test]
fn moving_into_an_occupied_folder_merges_rather_than_clobbers() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(new.join("2026-08-01-0900-earlier")).unwrap();
    // Both folders have junk: never a conflict.
    fs::write(old.join(JUNK[0]), "old").unwrap();
    fs::write(new.join(JUNK[0]), "new").unwrap();
    let pointer = Pointer::ok();

    run(&old, &new, &RealFs, &pointer).expect("a non-colliding merge must succeed");

    assert!(!old.exists(), "only junk was left, so the old folder goes");
    assert_all_meetings_in(&new);
    assert!(new.join("2026-08-01-0900-earlier").is_dir());
    assert_eq!(fs::read_to_string(new.join(JUNK[0])).unwrap(), "new");
    assert_eq!(pointer.saved(), Some(new));
}

#[test]
fn a_name_collision_refuses_the_whole_move_rather_than_guessing_which_copy_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(new.join(MEETINGS[1])).unwrap();
    let pointer = Pointer::ok();

    let error = run(&old, &new, &RealFs, &pointer).expect_err("a same-named folder must refuse");

    assert_eq!(error.kind, "folder-conflict");
    assert_all_meetings_in(&old);
    assert_eq!(names(&new), [MEETINGS[1]]);
    assert!(
        fs::read_dir(new.join(MEETINGS[1]))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(pointer.saved(), None);
}

// --- a failure partway: everything stays in the old folder ----------------

#[test]
fn a_merge_that_fails_mid_copy_leaves_every_meeting_in_the_old_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(new.join("2026-08-01-0900-earlier")).unwrap();
    let pointer = Pointer::ok();
    // Two files per meeting, so the fifth copy is partway through the third.
    let fs_ops = Faulty {
        fail_after: Some(5),
        ..Faulty::default()
    };

    let error = run(&old, &new, &fs_ops, &pointer).expect_err("the copy fails");

    assert!(error.message.contains("No space left"), "{}", error.message);
    assert_all_meetings_in(&old);
    assert_no_meetings_in(&new);
    assert_eq!(
        names(&new),
        ["2026-08-01-0900-earlier"],
        "only what was there before"
    );
    assert_eq!(
        pointer.saved(),
        None,
        "the app keeps pointing at the old folder"
    );

    // And a retry is not refused over half-copied names.
    run(&old, &new, &RealFs, &pointer).expect("the retry succeeds");
    assert_all_meetings_in(&new);
}

#[test]
fn a_cross_volume_move_that_fails_mid_copy_leaves_no_new_folder_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("other-volume").join("Meetings");
    let pointer = Pointer::ok();
    let fs_ops = Faulty {
        cross_volume: true,
        fail_after: Some(5),
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &pointer).expect_err("the copy fails");

    assert_all_meetings_in(&old);
    assert!(!new.exists(), "the half copy is taken back out");
    assert!(
        !tmp.path().join("other-volume").exists(),
        "and so is the parent the move made for it"
    );
    assert_eq!(pointer.saved(), None);
}

#[test]
fn a_copy_shorter_than_the_original_is_caught_before_anything_is_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    let pointer = Pointer::ok();
    let fs_ops = Faulty {
        cross_volume: true,
        short_copy: true,
        ..Faulty::default()
    };

    let error = run(&old, &new, &fs_ops, &pointer).expect_err("the check catches it");

    assert!(
        error.message.contains("does not match"),
        "{}",
        error.message
    );
    assert_all_meetings_in(&old);
    assert!(!new.exists());
    assert_eq!(pointer.saved(), None);
}

// --- the pointer cannot be saved ------------------------------------------

#[test]
fn a_pointer_that_fails_after_a_rename_renames_the_folder_back() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");

    run(&old, &new, &RealFs, &Pointer::failing()).expect_err("the pointer fails");

    assert_all_meetings_in(&old);
    assert!(!new.exists());
}

#[test]
fn a_pointer_that_fails_after_a_cross_volume_copy_keeps_the_old_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("other-volume").join("Meetings");
    let fs_ops = Faulty {
        cross_volume: true,
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &Pointer::failing()).expect_err("the pointer fails");

    assert_all_meetings_in(&old);
    assert!(!tmp.path().join("other-volume").exists());
}

#[test]
fn a_pointer_that_fails_after_a_merge_takes_back_only_the_copies() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(new.join("2026-08-01-0900-earlier")).unwrap();

    run(&old, &new, &RealFs, &Pointer::failing()).expect_err("the pointer fails");

    assert_all_meetings_in(&old);
    assert_eq!(names(&new), ["2026-08-01-0900-earlier"]);
}

#[test]
fn a_pointer_that_fails_with_nothing_to_move_removes_the_folder_it_made() {
    let tmp = tempfile::tempdir().unwrap();
    let new = tmp.path().join("made").join("Meetings");

    point_at(&new, &|root| Pointer::failing().save(root)).expect_err("the pointer fails");

    assert!(!tmp.path().join("made").exists());
}

// --- after the pointer is saved -------------------------------------------

#[test]
fn originals_that_cannot_be_deleted_are_leftovers_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(&new).unwrap();
    let pointer = Pointer::ok();
    let fs_ops = Faulty {
        keep_originals: true,
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &pointer).expect("the move itself worked");

    assert_all_meetings_in(&new);
    assert_eq!(
        pointer.saved(),
        Some(new),
        "the app points at the full copy"
    );
    assert!(old.join(MEETINGS[0]).join("transcript.md").is_file());
}

#[test]
fn a_meeting_that_lands_in_the_old_folder_mid_move_is_never_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("new");
    fs::create_dir_all(&new).unwrap();
    let pointer = Pointer::ok();
    let fs_ops = Faulty {
        arrives_in: Some(old.clone()),
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &pointer).expect("the move works");

    assert_all_meetings_in(&new);
    assert_eq!(
        names(&old),
        ["2026-09-05-0900-downloading"],
        "left, with its folder"
    );
}

#[test]
fn a_symlink_is_copied_as_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let elsewhere = tmp.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let link = old.join(MEETINGS[0]).join("shared");
    if let Err(error) = platform::symlink(&elsewhere, &link, true) {
        // Windows without Developer Mode cannot make one at all.
        eprintln!("skipped: cannot make a symlink here: {error}");
        return;
    }
    let new = tmp.path().join("new");
    let fs_ops = Faulty {
        cross_volume: true,
        ..Faulty::default()
    };

    run(&old, &new, &fs_ops, &Pointer::ok()).expect("the move works");

    let copied = new.join(MEETINGS[0]).join("shared");
    assert!(
        fs::symlink_metadata(&copied)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&copied).unwrap(), elsewhere);
    assert!(
        elsewhere.is_dir(),
        "what the link points at is never touched"
    );
}

// --- the path checks ------------------------------------------------------

#[test]
fn a_relative_folder_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let error = paths::checked(&old, Path::new("Meetings")).unwrap_err();
    assert_eq!(error.kind, "relative-folder");
    let dotted = tmp.path().join("new").join("..").join("old").join("sub");
    assert_eq!(
        paths::checked(&old, &dotted).unwrap_err().kind,
        "relative-folder"
    );
}

#[test]
fn a_folder_inside_the_current_one_is_refused_even_through_a_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let link = tmp.path().join("looks-elsewhere");
    if let Err(error) = platform::symlink(&old, &link, true) {
        eprintln!("skipped: cannot make a symlink here: {error}");
        return;
    }

    let nested = paths::checked(&old, &link.join("sub")).unwrap_err();
    assert_eq!(nested.kind, "nested-folder");
    assert_eq!(paths::checked(&old, &link).unwrap_err().kind, "same-folder");
    let around = paths::checked(&link.join(MEETINGS[0]), tmp.path()).unwrap_err();
    assert_eq!(around.kind, "nested-folder");
}

#[test]
fn the_same_folder_in_another_case_is_refused_on_a_case_insensitive_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let shouted = tmp.path().join("OLD");
    if !shouted.exists() {
        eprintln!("skipped: this disk is case-sensitive");
        return;
    }
    assert_eq!(
        paths::checked(&old, &shouted).unwrap_err().kind,
        "same-folder"
    );
    assert_eq!(
        paths::checked(&old, &shouted.join("Sub")).unwrap_err().kind,
        "nested-folder"
    );
}

#[test]
fn a_new_folder_is_checked_by_where_it_will_really_be() {
    let tmp = tempfile::tempdir().unwrap();
    let old = old_folder(tmp.path());
    let new = tmp.path().join("not-yet").join("Meetings");
    let (real_old, real_new) = paths::checked(&old, &new).expect("a sibling is fine");
    assert_eq!(real_old, dunce::canonicalize(&old).unwrap());
    assert_eq!(
        real_new,
        dunce::canonicalize(tmp.path())
            .unwrap()
            .join("not-yet")
            .join("Meetings")
    );
}

// --- the pointer file -----------------------------------------------------

#[test]
fn a_missing_pointer_means_the_default_folder() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(read_pointer(&tmp.path().join("root.json")).unwrap(), None);
}

#[test]
fn a_damaged_pointer_is_an_error_not_a_quiet_fall_back() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("root.json");
    // What a crash halfway through the old, non-atomic write left behind.
    fs::write(&path, "{\n  \"customRoot\": \"/Volumes/Da").unwrap();
    let error = read_pointer(&path).unwrap_err();
    assert_eq!(error.kind, POINTER_UNREADABLE);
    assert!(error.message.contains("root.json"), "{}", error.message);
}

#[test]
fn the_pointer_is_written_whole_and_reads_back() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config").join("root.json");
    let chosen = tmp.path().join("My Meetings");

    write_pointer_at(&path, &pointer_body(&chosen).unwrap()).unwrap();
    assert_eq!(read_pointer(&path).unwrap(), Some(chosen.clone()));

    // A second save replaces it, and leaves no temp file next to it.
    let again = tmp.path().join("Other");
    write_pointer_at(&path, &pointer_body(&again).unwrap()).unwrap();
    assert_eq!(read_pointer(&path).unwrap(), Some(again));
    assert_eq!(names(&tmp.path().join("config")), ["root.json"]);
}

#[test]
fn a_pointer_write_that_fails_is_reported_and_keeps_the_old_pointer() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("root.json");
    let old = tmp.path().join("Old");
    write_pointer_at(&path, &pointer_body(&old).unwrap()).unwrap();

    // A folder in the way of the new file: the rename onto it fails.
    let blocked = tmp.path().join("blocked").join("root.json");
    fs::create_dir_all(&blocked).unwrap();
    assert!(write_pointer_at(&blocked, b"{}").is_err());
    assert!(blocked.is_dir());
    assert_eq!(read_pointer(&path).unwrap(), Some(old));
}

#[test]
fn an_empty_pointer_means_the_default_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("root.json");
    let body = serde_json::to_vec(&RootPointer { custom_root: None }).unwrap();
    fs::write(&path, body).unwrap();
    assert_eq!(read_pointer(&path).unwrap(), None);
    fs::write(&path, r#"{"customRoot":""}"#).unwrap();
    assert_eq!(read_pointer(&path).unwrap(), None);
}
