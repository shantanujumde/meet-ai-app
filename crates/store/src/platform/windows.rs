//! Windows half of the `store` seam (see `mod.rs`).

/// `ERROR_SHARING_VIOLATION` (32) and `ERROR_LOCK_VIOLATION` (33): a player
/// or a backup tool holding the WAV open without delete sharing.
pub(crate) fn is_lock_violation(code: i32) -> bool {
    meeting_format::is_lock_violation(code)
}

/// Windows' `ReadDirectoryChangesW` watches a tree with one handle; there is
/// no per-folder limit to run out of.
pub(crate) fn is_out_of_watches(_code: i32) -> bool {
    false
}

/// `FILE_ATTRIBUTE_HIDDEN`.
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
/// `INVALID_FILE_ATTRIBUTES`: `GetFileAttributesW` failed.
const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFileAttributesW(name: *const u16) -> u32;
    fn SetFileAttributesW(name: *const u16, attributes: u32) -> i32;
}

/// Set the hidden attribute on `.app`, which Explorer otherwise shows.
pub(crate) fn hide_app_dir(dir: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt as _;
    if !dir.is_dir() {
        return;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives both calls.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        tracing::warn!(error = %std::io::Error::last_os_error(), path = %dir.display(), "could not read the attributes of the app folder");
        return;
    }
    if attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
        return;
    }
    // SAFETY: as above.
    if unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) } == 0 {
        tracing::warn!(error = %std::io::Error::last_os_error(), path = %dir.display(), "could not hide the app folder");
    }
}

#[cfg(test)]
pub(crate) mod test_lock {
    //! A WAV another program has open without delete sharing is refused with
    //! a sharing violation.

    use std::fs::File;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::path::{Path, PathBuf};

    /// Keeps `audio/mic.wav` open with no sharing until dropped.
    pub(crate) struct Held {
        _file: File,
        locked: Vec<PathBuf>,
    }

    impl Held {
        /// The WAVs a delete must skip while this is held.
        pub(crate) fn locked(&self) -> &[PathBuf] {
            &self.locked
        }
    }

    /// Lock `<audio>/mic.wav`. Always takes effect on Windows.
    pub(crate) fn hold(audio: &Path) -> Option<Held> {
        let wav = audio.join("mic.wav");
        let file = File::options().read(true).share_mode(0).open(&wav).unwrap();
        Some(Held {
            _file: file,
            locked: vec![wav],
        })
    }

    #[test]
    fn sharing_and_lock_violations_are_lock_violations() {
        assert!(super::is_lock_violation(32));
        assert!(super::is_lock_violation(33));
        assert!(!super::is_lock_violation(5));
        assert!(!super::is_lock_violation(2));
    }
}

#[cfg(test)]
mod hide_tests {
    use std::os::windows::fs::MetadataExt as _;

    #[test]
    fn the_app_folder_is_hidden_and_a_missing_one_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join(".app");
        super::hide_app_dir(&app);
        assert!(!app.exists());
        std::fs::create_dir(&app).unwrap();
        super::hide_app_dir(&app);
        let attributes = std::fs::metadata(&app).unwrap().file_attributes();
        assert_ne!(attributes & super::FILE_ATTRIBUTE_HIDDEN, 0);
    }

    #[test]
    fn the_watcher_hides_the_app_folder() {
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join(".app");
        std::fs::create_dir(&app).unwrap();
        let _watcher =
            crate::watcher::Watcher::start(root.path(), Default::default(), |_| {}).unwrap();
        let attributes = std::fs::metadata(&app).unwrap().file_attributes();
        assert_ne!(attributes & super::FILE_ATTRIBUTE_HIDDEN, 0);
    }
}
