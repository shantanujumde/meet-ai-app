//! Finding and placing the pretend agent CLI, `src/bin/fake_cli.rs` (TUR-54).
//!
//! The program works the same on every OS, so the agent tests that start a
//! "CLI" run on Windows too. See the program's own header for its settings.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Tells `fake-cli` to copy itself to this path and exit; see
/// [`FakeCli::install`]. Same name as in `src/bin/fake_cli.rs`.
const INSTALL_TO: &str = "FAKE_CLI_INSTALL_TO";

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
    ///
    /// The copy is written by a `fake-cli` process of its own, never by this
    /// one (TUR-96). A file this process had open for writing could be
    /// inherited by a child another test thread forks at that moment, and
    /// until that child execs, Linux refuses to run the file with
    /// `ETXTBSY` ("Text file busy"). That was the flaky
    /// `codex_printing_garbage_is_invalid_json`. The helper process has
    /// exited, and its file closed, before this returns, so nothing in the
    /// test process can hold the copy open.
    pub fn install(dir: &Path, name: &str) -> Self {
        let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        // quality: allow-unwrap test helper: a failure here should fail the test
        std::fs::create_dir_all(dir).expect("could not make the fake CLI's folder");
        let status = std::process::Command::new(fake_cli_path())
            .env(INSTALL_TO, &path)
            .stdin(std::process::Stdio::null())
            .status();
        assert!(
            matches!(status, Ok(s) if s.success()),
            "could not copy fake-cli to {}: {status:?}",
            path.display()
        );
        let config = dir.join(format!("{name}.fake"));
        // quality: allow-unwrap test helper: a failure here should fail the test
        std::fs::create_dir_all(&config).expect("could not make the fake CLI's settings");
        Self { path, config }
    }

    /// Where the copy is; the path to hand to the code under test.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets one setting (`stdout`, `code`, `sleep`, ...).
    pub fn set(&self, name: &str, value: impl AsRef<[u8]>) -> &Self {
        // quality: allow-unwrap test helper: a failure here should fail the test
        std::fs::write(self.config.join(name), value).expect("could not write a fake CLI setting");
        self
    }
}

/// Waits up to 10 s for `fake-cli` to write its grandchild's pid to
/// `pid_file` (setting `grandchild_pid_file`), and returns it.
///
/// # Panics
///
/// When no pid shows up in time.
pub fn wait_for_pid_file(pid_file: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // An empty file is one `fake-cli` has not finished writing.
        if let Ok(text) = std::fs::read_to_string(pid_file)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "no pid in {} after 10 s",
            pid_file.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the process `pid` is gone, waiting up to `within` for it to go. A
/// zombie (unix: dead, waiting for its new parent to reap it) counts as gone.
pub fn process_is_gone(pid: u32, within: Duration) -> bool {
    use sysinfo::{Pid, ProcessStatus, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    let deadline = Instant::now() + within;
    loop {
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        match system.process(pid) {
            None => return true,
            Some(p) if p.status() == ProcessStatus::Zombie => return true,
            Some(_) if Instant::now() >= deadline => return false,
            Some(_) => std::thread::sleep(Duration::from_millis(20)),
        }
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

    /// TUR-96: copies installed and run on many threads at once. With the
    /// copy written by this process, a fork on one thread inherited the
    /// half-written copy of another and Linux failed its exec with
    /// `ETXTBSY` now and then.
    #[test]
    fn copies_installed_and_run_on_many_threads_at_once_all_start() {
        let dir = tempfile_dir();
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        let cli = FakeCli::install(&dir, &format!("cli-{t}-{i}"));
                        cli.set("stdout", "ok");
                        let out = Command::new(cli.path())
                            .stdin(Stdio::null())
                            .output()
                            .unwrap_or_else(|e| panic!("thread {t} run {i}: {e}"));
                        assert_eq!(out.stdout, b"ok", "thread {t} run {i}: {out:?}");
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
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
