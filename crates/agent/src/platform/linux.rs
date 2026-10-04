//! Agent CLIs on Linux: the shared unix process-group and executable code.

pub(crate) use super::unix::{is_executable, wrap_tree};

// Finding the CLIs (TUR-53).

use std::path::{Path, PathBuf};

/// File name endings tried after the bare command name: none on unix.
pub(crate) const EXE_SUFFIXES: &[&str] = &[""];

/// The shell used when `$SHELL` is unset or not absolute.
const DEFAULT_SHELL: &str = "/bin/sh";

/// The login shell to ask `command -v`: `$SHELL`, else `/bin/sh`.
pub(crate) fn login_shell() -> Option<PathBuf> {
    Some(super::unix::shell_from_env().unwrap_or_else(|| PathBuf::from(DEFAULT_SHELL)))
}

/// Install folders in search order: the native installer's `~/.local/bin`,
/// npm's user prefix, cargo, Linuxbrew (system and per-user), then
/// `/usr/local/bin`. No `.app` bundles on Linux.
pub(crate) fn search_dirs(home: Option<&Path>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut bin_dirs = Vec::new();
    if let Some(home) = home {
        for rel in [".local/bin", ".npm-global/bin", ".cargo/bin"] {
            bin_dirs.push(home.join(rel));
        }
    }
    bin_dirs.push("/home/linuxbrew/.linuxbrew/bin".into());
    if let Some(home) = home {
        bin_dirs.push(home.join(".linuxbrew/bin"));
    }
    bin_dirs.push("/usr/local/bin".into());
    (bin_dirs, Vec::new())
}

/// [`search_dirs`] for a fake home, for tests.
#[cfg(test)]
pub(crate) fn search_dirs_under(home: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    search_dirs(Some(home))
}

/// Gives a test file the execute bits.
#[cfg(test)]
pub(crate) use super::unix::make_executable;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_folders_cover_each_installer() {
        let (bins, apps) = search_dirs(Some(Path::new("/home/u")));
        assert_eq!(
            bins,
            [
                PathBuf::from("/home/u/.local/bin"),
                "/home/u/.npm-global/bin".into(),
                "/home/u/.cargo/bin".into(),
                "/home/linuxbrew/.linuxbrew/bin".into(),
                "/home/u/.linuxbrew/bin".into(),
                "/usr/local/bin".into()
            ]
        );
        assert!(apps.is_empty());
    }
}
