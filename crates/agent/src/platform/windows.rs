//! Agent CLIs on Windows (and any other non-unix OS).
//!
//! The child starts in a Job Object of its own, so ending the job reaches
//! everything it starts: a `.cmd` shim runs under `cmd.exe`, and killing that
//! one process would leave its `node` (and the MCP servers `node` started)
//! running. The child starts suspended and is resumed only once it is in
//! process-wrap's job (which Cancel ends) and in a kill-on-close job of our
//! own (`windows_job.rs`, so a crashed app takes the tree with it). `CREATE_NO_WINDOW` keeps a
//! console window from flashing up for a console program started from the
//! app.

use std::path::{Path, PathBuf};

use process_wrap::std::{CommandWrap, CreationFlags, JobObject};
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

pub(crate) use super::windows_job::{TreeGuard, guard_tree};

/// Starts the child suspended, in a new Job Object, with no console window.
/// With `CREATE_SUSPENDED` set here, process-wrap leaves resuming it to
/// [`guard_tree`].
pub(crate) fn wrap_tree(command: &mut CommandWrap) {
    command.wrap(CreationFlags(CREATE_NO_WINDOW | CREATE_SUSPENDED));
    command.wrap(JobObject);
}

/// A regular file whose name ends in a program extension Windows runs
/// (`.exe`, `.cmd`, `.bat`, `.com`): execute permission is not a file bit
/// here. A bare `claude` next to `claude.cmd` (npm writes both; the bare one
/// is a shell script for Git Bash) does not count.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.is_file()
        && path.extension().is_some_and(|ext| {
            EXE_SUFFIXES
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known.trim_start_matches('.')))
        })
}

// Finding the CLIs (TUR-53).

/// File name endings tried after the command name, in order: the native
/// installer's `.exe`, then npm's `.cmd` shim, then `.bat` and `.com`.
pub(crate) const EXE_SUFFIXES: &[&str] = &[".exe", ".cmd", ".bat", ".com"];

/// No login shell on Windows: GUI apps get the user's full `PATH` already.
pub(crate) fn login_shell() -> Option<PathBuf> {
    None
}

/// Install folders in search order, from this user's folders. No `.app`
/// bundles on Windows.
pub(crate) fn search_dirs(home: Option<&Path>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (
        known_dirs(
            home,
            dirs::data_dir().as_deref(),
            dirs::data_local_dir().as_deref(),
        ),
        Vec::new(),
    )
}

/// `%USERPROFILE%\.local\bin` (Claude Code's native installer),
/// `%APPDATA%\npm` and `%LOCALAPPDATA%\npm` (npm's global shims), and
/// `%LOCALAPPDATA%\Programs`.
fn known_dirs(
    home: Option<&Path>,
    app_data: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join(".local").join("bin"));
    }
    if let Some(app_data) = app_data {
        dirs.push(app_data.join("npm"));
    }
    if let Some(local) = local_app_data {
        dirs.push(local.join("npm"));
        dirs.push(local.join("Programs"));
    }
    dirs
}

/// [`search_dirs`] with AppData under `home`, as Windows lays it out, for
/// tests on a fake home.
#[cfg(test)]
pub(crate) fn search_dirs_under(home: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let app_data = home.join("AppData");
    (
        known_dirs(
            Some(home),
            Some(&app_data.join("Roaming")),
            Some(&app_data.join("Local")),
        ),
        Vec::new(),
    )
}

/// Nothing to do: a known extension is what makes a file runnable here.
#[cfg(test)]
pub(crate) fn make_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_folders_cover_each_installer() {
        let home = Path::new(r"C:\Users\u");
        let roaming = Path::new(r"C:\Users\u\AppData\Roaming");
        let local = Path::new(r"C:\Users\u\AppData\Local");
        assert_eq!(
            known_dirs(Some(home), Some(roaming), Some(local)),
            [
                home.join(".local").join("bin"),
                roaming.join("npm"),
                local.join("npm"),
                local.join("Programs"),
            ]
        );
    }

    #[test]
    fn only_program_extensions_are_executable() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["claude", "claude.cmd", "codex.EXE", "notes.txt"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert!(is_executable(&dir.path().join("claude.cmd")));
        assert!(is_executable(&dir.path().join("codex.EXE")));
        assert!(!is_executable(&dir.path().join("claude")));
        assert!(!is_executable(&dir.path().join("notes.txt")));
        assert!(!is_executable(&dir.path().join("missing.exe")));
    }
}
