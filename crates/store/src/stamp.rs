//! A file's size and modified time, for the caches that skip files that did
//! not change (TUR-166): the meeting list's folder summaries and the highest
//! ticket number per meeting.

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// How long after a change a file's stamp is trusted. A second change within
/// the same clock tick (seconds on some network and FAT disks) can leave size
/// and time as they were, so a file changed this close to when it was looked
/// at is looked at again next time, as git does for its index.
const SETTLE: Duration = Duration::from_secs(2);

/// What a file looked like when it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    /// The stamp of `path`; `None` when it does not exist.
    pub(crate) fn of(path: &Path) -> io::Result<Option<Self>> {
        match std::fs::metadata(path) {
            Ok(meta) => Ok(Some(Self {
                len: meta.len(),
                modified: meta.modified().ok(),
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether this stamp, taken at `taken`, can stand for the file's
    /// contents later: its time is known and at least [`SETTLE`] older.
    fn settled(&self, taken: SystemTime) -> bool {
        self.modified.is_some_and(|modified| {
            taken
                .duration_since(modified)
                .is_ok_and(|age| age >= SETTLE)
        })
    }
}

/// Stamps taken together, and when. A cache entry made with them is reused
/// only while [`Stamps::still`] says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamps {
    files: Vec<Option<Stamp>>,
    taken: SystemTime,
}

impl Stamps {
    /// The stamps of `paths`, in order. `Err` when one cannot be looked at;
    /// such a result is not cached.
    pub(crate) fn of(paths: &[&Path]) -> io::Result<Self> {
        let taken = SystemTime::now();
        let files = paths
            .iter()
            .map(|path| Stamp::of(path))
            .collect::<io::Result<_>>()?;
        Ok(Self { files, taken })
    }

    /// Whether what was read when `self` was taken still holds for files
    /// that now look like `now`: nothing changed, and every file had settled.
    pub(crate) fn still(&self, now: &Self) -> bool {
        self.files == now.files
            && self
                .files
                .iter()
                .flatten()
                .all(|stamp| stamp.settled(self.taken))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(path: &Path, by: Duration) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(SystemTime::now() - by))
            .expect("set mtime");
    }

    #[test]
    fn an_old_unchanged_file_still_holds_and_a_fresh_one_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (old, missing) = (dir.path().join("old"), dir.path().join("missing"));
        std::fs::write(&old, "x").expect("write");
        age(&old, Duration::from_secs(60));
        let first = Stamps::of(&[&old, &missing]).expect("stamps");
        assert!(first.still(&Stamps::of(&[&old, &missing]).expect("again")));

        std::fs::write(&old, "xy").expect("grow");
        let grown = Stamps::of(&[&old, &missing]).expect("grown");
        assert!(!first.still(&grown), "a changed file is read again");
        assert!(
            !grown.still(&Stamps::of(&[&old, &missing]).expect("again")),
            "a file changed just now is read again, even unchanged since"
        );
        std::fs::write(&missing, "").expect("create");
        age(&old, Duration::from_secs(60));
        let both = Stamps::of(&[&old, &missing]).expect("both");
        assert!(!first.still(&both), "a file that appeared is a change");
    }
}
