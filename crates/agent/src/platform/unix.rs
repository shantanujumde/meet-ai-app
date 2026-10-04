//! Process groups and executables, shared by every unix.

use std::path::Path;

use process_wrap::std::{CommandWrap, ProcessGroup};

/// Puts the child at the head of a new process group of its own, so one
/// `killpg` reaches everything it starts (`process::ProcessTree`).
pub(crate) fn wrap_tree(command: &mut CommandWrap) {
    command.wrap(ProcessGroup::leader());
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
