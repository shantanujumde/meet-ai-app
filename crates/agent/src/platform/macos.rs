//! Agent CLIs on macOS: the shared unix process-group and executable code.

pub(crate) use super::unix::{TreeGuard, guard_tree, is_executable, wrap_tree};

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

/// Finding the CLIs inside macOS app bundles and the Claude desktop app's
/// support folder: paths that only exist on a Mac, so these run here only.
/// Moved from `detect.rs`'s tests in TUR-54.
#[cfg(test)]
mod bundle_tests {
    use std::fs;

    use crate::detect::tests::{CLAUDE_NEW, FakeMac};
    use crate::detect::{CLAUDE, CODEX, detect, find};

    #[test]
    fn codex_is_found_inside_chatgpt_app() {
        let mac = FakeMac::new();
        let codex = mac.cli(
            "apps/ChatGPT.app/Contents/Resources/codex",
            "--version\t0\tcodex-cli 0.50.0\\n\t\nlogin status\t0\t\tLogged in using ChatGPT\\n",
        );

        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert_eq!(install.path, codex);
        assert_eq!(install.version.as_deref(), Some("codex-cli 0.50.0"));
        assert!(install.signed_in);
    }

    #[test]
    fn codex_is_found_inside_a_standalone_codex_app() {
        let mac = FakeMac::new();
        let codex = mac.cli("apps/Codex.app/Contents/Resources/codex", "*\t0\t\t");
        assert_eq!(find(&CODEX, &mac.lookup()), Some(codex));
    }

    #[test]
    fn claude_desktop_copy_is_found_and_the_newest_wins() {
        let mac = FakeMac::new();
        let root = "home/Library/Application Support/Claude/claude-code";
        for version in ["2.1.9", "2.1.284", "not-a-version"] {
            mac.cli(
                &format!("{root}/{version}/claude.app/Contents/MacOS/claude"),
                CLAUDE_NEW,
            );
        }
        // A newer folder with no binary in it is skipped.
        fs::create_dir_all(mac.path(&format!("{root}/3.0.0"))).unwrap();

        assert_eq!(
            find(&CLAUDE, &mac.lookup()),
            Some(mac.path(&format!("{root}/2.1.284/claude.app/Contents/MacOS/claude")))
        );
    }

    #[test]
    fn installer_folders_come_before_app_bundles() {
        let mac = FakeMac::new();
        let in_bin = mac.cli("bin/claude", CLAUDE_NEW);
        mac.cli(
            "home/Library/Application Support/Claude/claude-code/2.1.284/claude.app/Contents/MacOS/claude",
            CLAUDE_NEW,
        );
        assert_eq!(find(&CLAUDE, &mac.lookup()), Some(in_bin));
    }
}
