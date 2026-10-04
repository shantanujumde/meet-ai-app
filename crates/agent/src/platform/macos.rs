//! Agent CLIs on macOS: the shared unix process-group and executable code.

pub(crate) use super::unix::{is_executable, wrap_tree};

// Finding the CLIs (TUR-53). The lists are unchanged from before the port.

use std::path::{Path, PathBuf};

/// File name endings tried after the bare command name: none on unix.
pub(crate) const EXE_SUFFIXES: &[&str] = &[""];

/// The shell used when `$SHELL` is unset or not absolute: the macOS default.
const DEFAULT_SHELL: &str = "/bin/zsh";

/// The login shell to ask `command -v`: `$SHELL`, else zsh.
pub(crate) fn login_shell() -> Option<PathBuf> {
    Some(super::unix::shell_from_env().unwrap_or_else(|| PathBuf::from(DEFAULT_SHELL)))
}

/// Install folders (`~/.local/bin`, Homebrew) and the folders holding `.app`
/// bundles (`/Applications`, `~/Applications`), in search order.
pub(crate) fn search_dirs(home: Option<&Path>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut bin_dirs = Vec::new();
    let mut app_dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = home {
        bin_dirs.push(home.join(".local/bin"));
        app_dirs.push(home.join("Applications"));
    }
    bin_dirs.push("/opt/homebrew/bin".into());
    bin_dirs.push("/usr/local/bin".into());
    (bin_dirs, app_dirs)
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
    fn mac_folders_are_the_ones_from_before_the_port() {
        let (bins, apps) = search_dirs(Some(Path::new("/Users/u")));
        assert_eq!(
            bins,
            [
                PathBuf::from("/Users/u/.local/bin"),
                "/opt/homebrew/bin".into(),
                "/usr/local/bin".into()
            ]
        );
        assert_eq!(
            apps,
            [
                PathBuf::from("/Applications"),
                "/Users/u/Applications".into()
            ]
        );
    }
}
