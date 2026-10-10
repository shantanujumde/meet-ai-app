//! One read of `config.jsonc` for a batch of section readers (TUR-171).
//!
//! Every section reader finds the meetings root (the root pointer, on disk),
//! then reads `config.jsonc`. Settings used to ask a dozen of them apart, so
//! the same two files were read a dozen times per visit. [`read_once`] reads
//! both once and hands that text to every reader called inside it, on this
//! thread only. Outside it nothing is held: each reader reads the disk as
//! before, so a change by hand is seen by the next read.
//!
//! Writes never see the held text: [`super::file`] reads the file fresh under
//! its write lock, so a save cannot start from a copy older than the disk.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::UiError;

/// What [`read_once`] read: the app folder (or why it could not be found),
/// and `config.jsonc` in it. The read error is kept as its kind and words,
/// since `io::Error` cannot be cloned.
struct Held {
    app_dir: Result<PathBuf, UiError>,
    file: Option<(PathBuf, HeldText)>,
}

/// The file's text, or the kind and words of why it could not be read.
type HeldText = Result<String, (io::ErrorKind, String)>;

thread_local! {
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
}

/// Puts back what was held before, however `read` ends (a panic included).
struct Restore(Option<Held>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        HELD.with(|held| *held.borrow_mut() = previous);
    }
}

/// Run `read` with the app folder and `config.jsonc` read once, up front.
/// Every section reader `read` calls on this thread uses that one read.
pub fn read_once<T>(read: impl FnOnce() -> T) -> T {
    let app_dir = super::find_app_dir();
    let file = app_dir.as_ref().ok().map(|dir| {
        let path = dir.join(super::FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|error| (error.kind(), error.to_string()));
        (path, text)
    });
    let previous = HELD.with(|held| held.replace(Some(Held { app_dir, file })));
    let _restore = Restore(previous);
    read()
}

/// The app folder [`read_once`] found, or `None` outside it.
pub(super) fn app_dir() -> Option<Result<PathBuf, UiError>> {
    HELD.with(|held| held.borrow().as_ref().map(|held| held.app_dir.clone()))
}

/// The text of the file at `path`: the held copy when [`read_once`] read
/// that same file, else read now.
pub(super) fn read(path: &Path) -> io::Result<String> {
    let held = HELD.with(|held| {
        held.borrow()
            .as_ref()
            .and_then(|held| held.file.clone())
            .filter(|(held_path, _)| held_path == path)
            .map(|(_, text)| text)
    });
    match held {
        Some(Ok(text)) => Ok(text),
        Some(Err((kind, words))) => Err(io::Error::new(kind, words)),
        None => std::fs::read_to_string(path),
    }
}

/// [`read_once`] with the app folder given, for tests.
#[cfg(test)]
pub(crate) fn read_once_in<T>(dir: &Path, read: impl FnOnce() -> T) -> T {
    let path = dir.join(super::FILE);
    let text = std::fs::read_to_string(&path).map_err(|error| (error.kind(), error.to_string()));
    let held = Held {
        app_dir: Ok(dir.to_path_buf()),
        file: Some((path, text)),
    };
    let previous = HELD.with(|slot| slot.replace(Some(held)));
    let _restore = Restore(previous);
    read()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_read_once_the_file_is_read_once_and_outside_it_every_time() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(super::super::FILE);
        std::fs::write(&path, "first").unwrap();

        read_once_in(temp.path(), || {
            assert_eq!(app_dir().unwrap().unwrap(), temp.path());
            // Changed on disk after the one read: the held copy still answers.
            std::fs::write(&path, "second").unwrap();
            assert_eq!(read(&path).unwrap(), "first");
            // Another file is read from disk.
            let other = temp.path().join("other.jsonc");
            std::fs::write(&other, "other").unwrap();
            assert_eq!(read(&other).unwrap(), "other");
        });

        assert!(app_dir().is_none());
        assert_eq!(read(&path).unwrap(), "second");
    }

    #[test]
    fn a_missing_file_is_held_as_not_found() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(super::super::FILE);
        read_once_in(temp.path(), || {
            std::fs::write(&path, "made later").unwrap();
            assert_eq!(read(&path).unwrap_err().kind(), io::ErrorKind::NotFound);
        });
    }

    #[test]
    fn the_section_readers_use_the_one_read() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(super::super::FILE);
        std::fs::write(
            &path,
            r#"{ "app": { "show_in_dock_when_closed": true }, "agent": { "auto_run": false } }"#,
        )
        .unwrap();
        read_once_in(temp.path(), || {
            std::fs::remove_file(&path).unwrap();
            assert!(super::super::app().show_in_dock_when_closed);
            assert!(!super::super::agent().unwrap().auto_run);
        });
    }

    #[test]
    fn what_was_held_before_comes_back_after_a_panic() {
        let temp = tempfile::tempdir().unwrap();
        let caught = std::panic::catch_unwind(|| read_once_in(temp.path(), || panic!("reader")));
        assert!(caught.is_err());
        assert!(app_dir().is_none());
    }
}
