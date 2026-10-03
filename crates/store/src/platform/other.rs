//! Every OS but Windows: the `store` seam with plain `std` (see `mod.rs`).

/// No extra codes: a held file is refused with `PermissionDenied`, which the
/// caller already counts, or not refused at all (Unix deletes an open file).
pub(crate) fn is_lock_violation(_code: i32) -> bool {
    false
}

#[cfg(test)]
pub(crate) mod test_lock {
    //! On Unix an open file can still be deleted, so the test makes the
    //! `audio/` folder read-only instead: the delete is refused with
    //! `PermissionDenied`.

    use std::fs::Permissions;
    use std::path::{Path, PathBuf};

    /// Keeps `audio/` read-only until dropped.
    pub(crate) struct Held {
        audio: PathBuf,
        writable: Permissions,
        locked: Vec<PathBuf>,
    }

    impl Held {
        /// The WAVs a delete must skip while this is held.
        pub(crate) fn locked(&self) -> &[PathBuf] {
            &self.locked
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.audio, self.writable.clone());
        }
    }

    /// Make `audio/` read-only. `None` when that does not stop a delete here
    /// (running as root ignores the folder's mode), so there is nothing to
    /// test.
    pub(crate) fn hold(audio: &Path) -> Option<Held> {
        let writable = std::fs::metadata(audio).unwrap().permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(audio, read_only).unwrap();
        let mut locked: Vec<PathBuf> = std::fs::read_dir(audio)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
            .collect();
        locked.sort();
        let held = Held {
            audio: audio.to_path_buf(),
            writable,
            locked,
        };
        let probe = audio.join(".probe");
        if std::fs::write(&probe, b"").is_ok() {
            let _ = std::fs::remove_file(&probe);
            return None;
        }
        Some(held)
    }

    #[test]
    fn no_raw_code_is_a_lock_violation() {
        // Windows' 32 and 33 are unrelated errors here (EPIPE and EDOM on Unix).
        for code in [2, 5, 13, 32, 33] {
            assert!(!super::is_lock_violation(code));
        }
    }
}
