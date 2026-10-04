//! Finding and placing the pretend agent CLI, `src/bin/fake_cli.rs` (TUR-54).
//!
//! The program works the same on every OS, so the agent tests that start a
//! "CLI" run on Windows too. See the program's own header for its settings.

use std::path::{Path, PathBuf};

/// The `fake-cli` program `build.rs` compiled for this test run.
///
/// # Panics
///
/// When the build target is not the host, where `build.rs` compiles nothing
/// (only `cargo check` builds for another target, and that never runs tests).
pub fn fake_cli_path() -> &'static Path {
    let path = env!("FAKE_CLI_PATH");
    assert!(
        !path.is_empty(),
        "fake-cli is only built when the target is the host"
    );
    Path::new(path)
}

/// A copy of `fake-cli` at a path of the test's choosing, set up through
/// files next to it rather than the environment, so tests running in
/// parallel never see each other's settings.
#[derive(Debug, Clone)]
pub struct FakeCli {
    path: PathBuf,
    config: PathBuf,
}

impl FakeCli {
    /// Copies `fake-cli` to `<dir>/<name>` (plus `.exe` on Windows) and makes
    /// its empty settings folder.
    pub fn install(dir: &Path, name: &str) -> Self {
        let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(dir).expect("could not make the fake CLI's folder");
        std::fs::copy(fake_cli_path(), &path).expect("could not copy fake-cli");
        let config = dir.join(format!("{name}.fake"));
        std::fs::create_dir_all(&config).expect("could not make the fake CLI's settings");
        Self { path, config }
    }

    /// Where the copy is; the path to hand to the code under test.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets one setting (`stdout`, `code`, `sleep`, ...).
    pub fn set(&self, name: &str, value: impl AsRef<[u8]>) -> &Self {
        std::fs::write(self.config.join(name), value).expect("could not write a fake CLI setting");
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn an_installed_copy_reads_its_own_settings() {
        let dir = tempfile_dir();
        let cli = FakeCli::install(&dir, "claude");
        cli.set("stdout", "hello\n")
            .set("stderr", "oops")
            .set("code", "3");
        let out = Command::new(cli.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(out.stderr, b"oops");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_environment_wins_and_stdin_can_be_echoed() {
        let out = Command::new(fake_cli_path())
            .env("FAKE_ECHO_STDIN", "1")
            .env("FAKE_EXPECT_ARGS", "a b")
            .args(["a", "b"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");

        let out = Command::new(fake_cli_path())
            .env("FAKE_EXPECT_ARGS", "a b")
            .arg("c")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(7));
    }

    /// A fresh folder under the system temp folder; this crate has no
    /// dependencies, so no `tempfile`.
    fn tempfile_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "test-support-fake-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
