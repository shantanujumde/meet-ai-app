//! `meeting-format` on Windows.

use std::io;
use std::path::Path;

/// A no-op: Windows has no directory handle to flush, and `MoveFileEx` is
/// already as durable as that platform offers.
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// `ERROR_ACCESS_DENIED`: what `MoveFileEx` returns when the target is open
/// without delete sharing.
const ERROR_ACCESS_DENIED: i32 = 5;
/// `ERROR_SHARING_VIOLATION`: another program has the file open.
const ERROR_SHARING_VIOLATION: i32 = 32;
/// `ERROR_LOCK_VIOLATION`: another program has a byte range locked.
const ERROR_LOCK_VIOLATION: i32 = 33;
/// The waits between tries. Antivirus scanners, the search indexer and
/// editors hold a file for a moment; together these give them ~310 ms.
const RENAME_BACKOFF_MS: [u64; 5] = [10, 20, 40, 80, 160];

/// `rename`, retried while another program briefly holds `to` (or `from`)
/// open. Any other error, or a hold that outlasts every try, is returned with
/// a message naming the file.
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    rename_with(from, to, &RENAME_BACKOFF_MS)
}

fn is_held(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
    )
}

fn rename_with(from: &Path, to: &Path, backoff_ms: &[u64]) -> io::Result<()> {
    let mut waits = backoff_ms.iter();
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if is_held(&error) => match waits.next() {
                Some(ms) => std::thread::sleep(std::time::Duration::from_millis(*ms)),
                None => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!(
                            "could not save {}: another program (an antivirus scan, the \
                             search indexer or an editor) kept it open through {} tries ({error})",
                            to.display(),
                            backoff_ms.len() + 1
                        ),
                    ));
                }
            },
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::time::Duration;

    /// FILE_SHARE_READ | FILE_SHARE_WRITE, no FILE_SHARE_DELETE: a rename
    /// over the file is refused while this handle is open.
    const SHARE_NO_DELETE: u32 = 0x1 | 0x2;

    fn hold(path: &std::path::Path) -> File {
        File::options()
            .read(true)
            .share_mode(SHARE_NO_DELETE)
            .open(path)
            .unwrap()
    }

    #[test]
    fn an_atomic_write_waits_out_a_brief_hold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, b"old").unwrap();
        let held = hold(&path);
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            drop(held);
        });
        crate::write_atomic(&path, b"new").unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn a_hold_that_never_ends_is_a_clear_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, b"old").unwrap();
        let _held = hold(&path);
        let error = crate::write_atomic(&path, b"new").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("notes.md"), "{message}");
        assert!(message.contains("another program"), "{message}");
        drop(_held);
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        // The failed write left no temp file behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
