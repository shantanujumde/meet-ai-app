//! Catching the index up with the folder after the app was closed (TUR-152).
//!
//! [`Index::open`] keeps a valid `index.db` as it is, and the watcher only
//! sees changes made while the app runs. Anything done in between (a sync
//! client bringing in another Mac's edits, an agent writing notes after the
//! app quit, a meeting deleted by hand) would stay out of search until the
//! next full rebuild. [`Index::catch_up`] compares every meeting folder's
//! newest change with the time its rows were written, and reads again only
//! the meetings that differ.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Index, index_error, reindex};
use crate::{Error, folder, is_plain_name};

impl Index {
    /// Re-read every meeting under `root` whose files changed since it was
    /// indexed, add the ones that are new and drop the ones whose folder is
    /// gone. Returns how many meetings that touched. Cheap when nothing
    /// changed: a few file times per meeting, no file is read.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when `root` cannot be listed, and [`Error::Index`] when
    /// the database cannot be read or written.
    pub fn catch_up(&mut self, root: &Path) -> Result<usize, Error> {
        let mut ids: BTreeSet<String> = folder::meeting_dirs(root)?
            .iter()
            .filter_map(|dir| dir.file_name()?.to_str().map(str::to_owned))
            .filter(|id| is_plain_name(id))
            .collect();
        let tx = self.conn.transaction().map_err(index_error)?;
        {
            let mut statement = tx.prepare("SELECT id FROM meetings").map_err(index_error)?;
            let known = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(index_error)?;
            for id in known {
                ids.insert(id.map_err(index_error)?);
            }
        }
        let mut changed = 0;
        for id in ids {
            changed += usize::from(reindex(&tx, root, &id, false)?);
        }
        tx.commit().map_err(index_error)?;
        Ok(changed)
    }
}
