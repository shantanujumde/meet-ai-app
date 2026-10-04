//! Process groups, executables and the shell, shared by every unix.

use std::path::Path;
use std::process::{Command, Stdio};

/// The system `kill`, by full path so `PATH` cannot swap it out.
pub(crate) const KILL: &str = "/bin/kill";

/// The shell the fake harness runs its script with.
#[cfg(any(test, feature = "test-support"))]
pub(crate) const POSIX_SHELL: Option<&str> = Some("/bin/sh");

/// Puts the child in a new process group of its own, so one kill reaches
/// everything it starts.
pub(crate) fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// Kills every process in the group the child leads. Runs the system `kill`,
/// so no `unsafe` and no extra crate is needed.
pub(crate) fn kill_group(pid: u32) {
    let _ = Command::new(KILL)
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A regular file with any execute bit set.
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `$SHELL` when it is an absolute path.
pub(crate) fn shell_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Gives a test file the execute bits.
#[cfg(test)]
pub(crate) fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
