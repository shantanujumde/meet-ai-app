//! Moving the meetings folder without ever leaving a meeting in two folders,
//! or in none (TUR-149).
//!
//! The order is the whole point:
//!
//! 1. copy every entry into the new folder, flushing each file and folder;
//! 2. check the copy: every entry is there, every file the size the copy wrote;
//! 3. save the new location (the pointer, `super::write_pointer_at`);
//! 4. only then delete the originals.
//!
//! A failure in steps 1 to 3 deletes what this move created in the new folder
//! (only that: whatever was there before stays) and leaves the old folder as
//! it was, so the app still points at a folder with every meeting in it. A
//! failure in step 4 is a warning, not an error: the app already points at the
//! full copy, and what could not be deleted is only a leftover.
//!
//! The one shortcut: an empty destination on the same volume is one `rename`
//! of the whole folder, then the pointer. If the pointer cannot be saved, the
//! folder is renamed back.
//!
//! Step 4 deletes only what was copied, never a recursive delete of the old
//! folder: a meeting that appeared there mid-move (iCloud or Dropbox still
//! downloading it) was not copied, so it is not deleted either, and the old
//! folder is left in place around it.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::UiError;
use crate::meetings::platform;

/// Files an OS drops into any folder it has shown. Never a reason to refuse a
/// merge, not carried over from the top of the old folder, and deleted when
/// the old folder is emptied.
pub(super) const JUNK: &[&str] = &[".DS_Store"];

fn is_junk(name: &OsStr) -> bool {
    JUNK.iter().any(|junk| name == *junk)
}

/// The steps of a move that can fail partway through, so a test can make one
/// fail. Making folders and reading listings are plain `std::fs`.
pub(super) trait MoveFs {
    /// Rename in one step. Fails across volumes, which is what sends a move
    /// down the copy path.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    /// Copy one file's bytes into a new file. Returns how many were written.
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<u64> {
        fs::copy(from, to)
    }

    /// Delete one original file, once the new location is saved.
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

/// The real filesystem.
pub(super) struct RealFs;

impl MoveFs for RealFs {}

/// Saves the new location. Called once, after the copy is complete and
/// checked; an `Err` undoes the move.
pub(super) type SavePointer<'a> = &'a dyn Fn(&Path) -> Result<(), UiError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Symlink,
}

/// One entry this move created in the new folder, and the original it copies.
struct Copied {
    from: PathBuf,
    to: PathBuf,
    kind: Kind,
    /// The bytes the copy wrote. Files only.
    len: u64,
}

/// Everything one move has created so far, in the order it was created, so a
/// failure takes exactly that back out and nothing else.
#[derive(Default)]
struct Transfer {
    /// Folders the move made to hold the new root, the new root included,
    /// shallowest first.
    made_dirs: Vec<PathBuf>,
    copied: Vec<Copied>,
}

/// No meetings to carry (the old folder does not exist): make `new_root` and
/// point at it. If the pointer cannot be saved, the folders just made go again.
pub(super) fn point_at(new_root: &Path, save: SavePointer<'_>) -> Result<(), UiError> {
    let made = make_dirs(new_root)?;
    save(new_root).inspect_err(|_| remove_made(&made))
}

/// Move every meeting in `old_root` to `new_root` and save `new_root` as the
/// meetings folder. `new_root` may already hold other things: the meetings are
/// merged in beside them, and a name both folders have refuses the move before
/// anything is copied.
///
/// Both paths are canonical (see `super::paths`).
pub(super) fn move_root(
    old_root: &Path,
    new_root: &Path,
    fs_ops: &dyn MoveFs,
    save: SavePointer<'_>,
) -> Result<(), UiError> {
    if !old_root.is_dir() {
        return point_at(new_root, save);
    }
    match fs::symlink_metadata(new_root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            into_new_folder(old_root, new_root, fs_ops, save)
        }
        Err(error) => Err(error.into()),
        Ok(meta) if meta.is_dir() => {
            refuse_conflicts(old_root, new_root)?;
            copy_then_switch(old_root, new_root, fs_ops, save, Transfer::default())
        }
        Ok(_) => Err(UiError::app(
            "not-a-folder",
            format!("\"{}\" is a file, not a folder.", new_root.display()),
        )),
    }
}

/// `new_root` does not exist yet: one rename if the volume allows it, else a
/// copy.
fn into_new_folder(
    old_root: &Path,
    new_root: &Path,
    fs_ops: &dyn MoveFs,
    save: SavePointer<'_>,
) -> Result<(), UiError> {
    let made = match new_root.parent() {
        Some(parent) => make_dirs(parent)?,
        None => Vec::new(),
    };
    if fs_ops.rename(old_root, new_root).is_ok() {
        let saved = new_root
            .parent()
            .map_or(Ok(()), meeting_format::sync_dir)
            .map_err(UiError::from)
            .and_then(|()| save(new_root));
        return saved.map_err(|error| rename_back(old_root, new_root, fs_ops, &made, error));
    }
    // `rename` refuses to cross volumes, so copy instead.
    let mut transfer = Transfer {
        made_dirs: made,
        copied: Vec::new(),
    };
    if let Err(error) = fs::create_dir(new_root) {
        transfer.roll_back();
        return Err(error.into());
    }
    transfer.made_dirs.push(new_root.to_path_buf());
    copy_then_switch(old_root, new_root, fs_ops, save, transfer)
}

/// The pointer could not be saved after a whole-folder rename: rename it back,
/// so the app's pointer and the meetings agree again.
fn rename_back(
    old_root: &Path,
    new_root: &Path,
    fs_ops: &dyn MoveFs,
    made: &[PathBuf],
    mut error: UiError,
) -> UiError {
    match fs_ops.rename(new_root, old_root) {
        Ok(()) => remove_made(made),
        Err(back) => {
            tracing::error!(
                error = %back,
                from = %new_root.display(),
                to = %old_root.display(),
                "the new meetings folder could not be saved and the move could not be undone"
            );
            error.message = format!(
                "{} Your meetings are all in \"{}\" now. Pick that folder as your meetings \
                 folder to see them again.",
                error.message,
                new_root.display()
            );
        }
    }
    error
}

/// Copy, check, save, and only then delete the originals. `transfer` holds the
/// folders already made for `new_root`, if any.
fn copy_then_switch(
    old_root: &Path,
    new_root: &Path,
    fs_ops: &dyn MoveFs,
    save: SavePointer<'_>,
    mut transfer: Transfer,
) -> Result<(), UiError> {
    let switched = copy_children(old_root, new_root, fs_ops, &mut transfer, true)
        .and_then(|()| new_root.parent().map_or(Ok(()), meeting_format::sync_dir))
        .and_then(|()| transfer.verify())
        .map_err(UiError::from)
        .and_then(|()| save(new_root));
    if let Err(error) = switched {
        tracing::warn!(
            error = %error.message,
            from = %old_root.display(),
            to = %new_root.display(),
            "the meetings folder move failed; taking back what it copied"
        );
        transfer.roll_back();
        return Err(error);
    }
    transfer.delete_originals(old_root, fs_ops);
    Ok(())
}

/// Copy everything in `from` into `to`, which exists. At the top of the old
/// folder (`top`), junk is skipped.
fn copy_children(
    from: &Path,
    to: &Path,
    fs_ops: &dyn MoveFs,
    transfer: &mut Transfer,
    top: bool,
) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if top && is_junk(&name) {
            continue;
        }
        copy_entry(&entry.path(), &to.join(&name), fs_ops, transfer)?;
    }
    meeting_format::sync_dir(to)
}

/// Copy one entry: a folder with everything in it, a file, or a symlink as a
/// symlink (never what it points at, which may be outside the folder or be
/// the folder itself).
fn copy_entry(
    from: &Path,
    to: &Path,
    fs_ops: &dyn MoveFs,
    transfer: &mut Transfer,
) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(from)?;
        let points_at_dir = fs::metadata(from).is_ok_and(|meta| meta.is_dir());
        platform::symlink(&target, to, points_at_dir)?;
        transfer.push(from, to, Kind::Symlink);
    } else if kind.is_dir() {
        fs::create_dir(to)?;
        transfer.push(from, to, Kind::Dir);
        copy_children(from, to, fs_ops, transfer, false)?;
    } else if kind.is_file() {
        // `fs::copy` overwrites, and nothing already there is ours to replace.
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", to.display()),
            ));
        }
        // Recorded before the copy, so a copy that dies halfway is still
        // taken back out.
        transfer.push(from, to, Kind::File);
        let written = fs_ops.copy_file(from, to)?;
        if written < meta.len() {
            return Err(io::Error::other(format!(
                "copied only {written} of {} bytes of {}",
                meta.len(),
                from.display()
            )));
        }
        if let Some(last) = transfer.copied.last_mut() {
            last.len = written;
        }
        platform::sync_file(to)?;
    } else {
        // A socket or a pipe: not meeting data, and not copyable. Left where
        // it is, which also keeps the old folder from being removed.
        tracing::warn!(path = %from.display(), "not a file, folder or link; left in the old meetings folder");
    }
    Ok(())
}

impl Transfer {
    fn push(&mut self, from: &Path, to: &Path, kind: Kind) {
        self.copied.push(Copied {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            kind,
            len: 0,
        });
    }

    /// Every copied entry is in the new folder, as the same kind, and every
    /// file holds exactly the bytes the copy wrote. A file that grows while it
    /// is copied (the app's own log) still passes: the copy wrote at least what
    /// the original had when it started.
    fn verify(&self) -> io::Result<()> {
        for item in &self.copied {
            let meta = fs::symlink_metadata(&item.to)?;
            let matches = match item.kind {
                Kind::File => meta.is_file() && meta.len() == item.len,
                Kind::Dir => meta.is_dir(),
                Kind::Symlink => meta.file_type().is_symlink(),
            };
            if !matches {
                return Err(io::Error::other(format!(
                    "the copy of {} at {} does not match it",
                    item.from.display(),
                    item.to.display()
                )));
            }
        }
        Ok(())
    }

    /// Take out everything this move created, newest first, and nothing else.
    /// A folder something else has written into since is left, not emptied.
    fn roll_back(&self) {
        for item in self.copied.iter().rev() {
            let removed = match item.kind {
                Kind::File => fs::remove_file(&item.to),
                Kind::Symlink => platform::remove_symlink(&item.to),
                Kind::Dir => fs::remove_dir(&item.to),
            };
            if let Err(error) = removed
                && error.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(%error, path = %item.to.display(), "could not take a partly moved entry back out");
            }
        }
        remove_made(&self.made_dirs);
    }

    /// Delete the originals of everything copied, newest first. Never an
    /// error: the new folder is already saved and complete.
    fn delete_originals(&self, old_root: &Path, fs_ops: &dyn MoveFs) {
        let mut left = 0_usize;
        for item in self.copied.iter().rev() {
            let removed = match item.kind {
                Kind::File => fs_ops.remove_file(&item.from),
                Kind::Symlink => platform::remove_symlink(&item.from),
                Kind::Dir => remove_emptied_dir(&item.from),
            };
            if let Err(error) = removed {
                left += 1;
                tracing::debug!(%error, path = %item.from.display(), "left in the old meetings folder");
            }
        }
        if let Err(error) = remove_emptied_dir(old_root) {
            tracing::warn!(
                %error,
                left,
                old = %old_root.display(),
                "the meetings moved, but the old folder still has files in it, so it was left in place"
            );
        }
    }
}

/// Remove a folder the move has emptied: known junk first, then the folder
/// only if nothing else is in it.
fn remove_emptied_dir(dir: &Path) -> io::Result<()> {
    for junk in JUNK {
        match fs::remove_file(dir.join(junk)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    fs::remove_dir(dir)
}

/// The conflict check for a merge: refuse outright on any name both folders
/// have, rather than guessing which of two same-named meetings is the real one.
fn refuse_conflicts(old_root: &Path, new_root: &Path) -> Result<(), UiError> {
    let mut conflicts = Vec::new();
    for entry in fs::read_dir(old_root)? {
        let name = entry?.file_name();
        if !is_junk(&name) && fs::symlink_metadata(new_root.join(&name)).is_ok() {
            conflicts.push(name.to_string_lossy().into_owned());
        }
    }
    if conflicts.is_empty() {
        return Ok(());
    }
    conflicts.sort();
    let noun = if conflicts.len() == 1 {
        "item"
    } else {
        "items"
    };
    Err(UiError::app(
        "folder-conflict",
        format!(
            "\"{}\" already has {noun} named the same as something in your current meetings \
             folder: {}. Rename or remove {noun} there first, then try again.",
            new_root.display(),
            conflicts.join(", "),
        ),
    ))
}

/// Make `dir` and any missing parents. Returns the folders made, shallowest
/// first, so a failure can remove exactly those.
fn make_dirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut at = Some(dir);
    while let Some(path) = at {
        if fs::symlink_metadata(path).is_ok() {
            break;
        }
        missing.push(path.to_path_buf());
        at = path.parent();
    }
    missing.reverse();
    for (made, path) in missing.iter().enumerate() {
        if let Err(error) = fs::create_dir(path) {
            remove_made(&missing[..made]);
            return Err(error);
        }
    }
    Ok(missing)
}

/// Remove folders [`make_dirs`] made, deepest first, if they are still empty.
fn remove_made(made: &[PathBuf]) {
    for dir in made.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
}
