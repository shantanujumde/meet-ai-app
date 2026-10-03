//! Windows half of the `store` seam (see `mod.rs`).

/// `ERROR_SHARING_VIOLATION` (32) and `ERROR_LOCK_VIOLATION` (33): a player
/// or a backup tool holding the WAV open without delete sharing.
pub(crate) fn is_lock_violation(code: i32) -> bool {
    matches!(code, 32 | 33)
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
