//! `notes.md` — the user's own notes, free-form markdown.
//!
//! No structure is imposed and nothing is parsed: whatever the user typed is
//! what comes back.
//!
//! A `notes.md` that is not UTF-8 is refused both ways rather than decoded
//! lossily. The notes pane autosaves whatever it was handed, so a lossy read
//! would turn the bad bytes into U+FFFD on screen and the next autosave would
//! make that permanent — the same "never overwrite what could not be read"
//! rule [`crate::meeting`] and [`crate::ticket`] follow.

use std::io;
use std::path::Path;

use crate::{Error, NOTES_FILE};

/// Read the notes in meeting folder `dir`. A missing file is an empty page,
/// not an error — a meeting nobody wrote notes on is the normal case.
///
/// Bytes that are not UTF-8 are an [`Error::Io`] of kind
/// [`io::ErrorKind::InvalidData`], which [`crate::folder::load`] shows as
/// `Problem::Unreadable` on `notes.md`.
pub fn read(dir: &Path) -> Result<String, Error> {
    let bytes = match std::fs::read(dir.join(NOTES_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error.into()),
    };
    String::from_utf8(bytes).map_err(|error| {
        Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{NOTES_FILE} is not UTF-8 text and was left untouched: {error}"),
        ))
    })
}

/// Save the notes in meeting folder `dir` atomically. The notes pane
/// autosaves while the user types, so a crash mid-save must never leave a
/// half-written file.
///
/// Refused, with the file left byte-identical, when the `notes.md` already
/// there cannot be read: not UTF-8 is the same `InvalidData` error [`read`]
/// gives, and any other read failure is returned as is. The pane was shown an
/// empty page for that file, so saving over it would replace notes the user
/// never saw.
pub fn write(dir: &Path, body: &str) -> Result<(), Error> {
    read(dir)?;
    crate::write_atomic(&dir.join(NOTES_FILE), body)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Problem;
    use crate::folder::{self, FileProblem};

    /// A scratch folder unique to this test and this process, emptied first.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("meet-ai-store-notes-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    fn is_invalid_data<T>(result: Result<T, Error>) -> bool {
        matches!(result, Err(Error::Io(error)) if error.kind() == io::ErrorKind::InvalidData)
    }

    #[test]
    fn missing_notes_are_an_empty_page() {
        let dir = scratch("missing");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read(&dir).unwrap(), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn notes_round_trip_and_leave_no_temp_file_behind() {
        let dir = scratch("round-trip");
        std::fs::create_dir_all(&dir).unwrap();
        let body = "# Follow-ups\n\n- ask about [TUR-17]: sessions\n";
        write(&dir, "first draft").unwrap();
        write(&dir, body).unwrap();
        assert_eq!(read(&dir).unwrap(), body);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(NOTES_FILE)]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_utf8_notes_are_an_error_and_the_folder_load_flags_them() {
        let dir = scratch("not-utf8-read");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(NOTES_FILE), b"caf\xe9 notes").unwrap();
        assert!(is_invalid_data(read(&dir)));

        let loaded = folder::load(&dir).unwrap();
        assert_eq!(loaded.notes, "");
        assert!(
            matches!(
                loaded.problems.as_slice(),
                [FileProblem { file, problem: Problem::Unreadable { .. } }] if file == NOTES_FILE
            ),
            "{:?}",
            loaded.problems
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writing_over_non_utf8_notes_is_refused_and_the_bytes_are_unchanged() {
        let dir = scratch("not-utf8-write");
        std::fs::create_dir_all(&dir).unwrap();
        let bytes: &[u8] = b"caf\xe9 notes";
        std::fs::write(dir.join(NOTES_FILE), bytes).unwrap();
        assert!(is_invalid_data(write(&dir, "autosave")));
        assert_eq!(std::fs::read(dir.join(NOTES_FILE)).unwrap(), bytes);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(NOTES_FILE)]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
