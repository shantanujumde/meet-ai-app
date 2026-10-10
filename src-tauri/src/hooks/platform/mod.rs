//! Which shell runs a hook (TUR-63), the hooks' OS seam (R10).
//!
//! The choice itself is [`invocation`], plain data tested on every OS.
//! Only [`command`] touches the OS, for the one thing std cannot do portably:
//! hand `cmd.exe` its command line as written.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// The `$0` a unix hook sees, so `"$@"` holds only the meeting folder.
const SH_NAME: &str = "meet-ai-hook";

/// How to start one hook, before it is a `Command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: &'static str,
    /// Passed one by one, quoted by std as usual.
    pub args: Vec<OsString>,
    /// Windows `cmd /C` only: the rest of the command line, as is.
    pub raw_tail: Option<OsString>,
}

/// The shell for `command` on this OS: `sh -c` on macOS and Linux, `cmd /C`
/// on Windows, or PowerShell when the command is a `.ps1` file. The meeting
/// folder is always the last argument.
pub fn invocation(windows: bool, command: &str, meeting_dir: &Path) -> Invocation {
    if !windows {
        // `"$@"` is the folder, quoted by the shell, never parsed as code.
        return Invocation {
            program: "sh",
            args: vec![
                "-c".into(),
                format!("{command} \"$@\"").into(),
                SH_NAME.into(),
                meeting_dir.into(),
            ],
            raw_tail: None,
        };
    }
    let script = command.trim().trim_matches('"');
    if script.to_ascii_lowercase().ends_with(".ps1") {
        return Invocation {
            program: "powershell",
            args: vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script.into(),
                meeting_dir.into(),
            ],
            raw_tail: None,
        };
    }
    // `/S /C "<line>"`: cmd strips the outer quotes and runs the line as
    // typed. A Windows path cannot hold a `"`, so quoting the folder is safe.
    let mut line = OsString::from("\"");
    line.push(command);
    line.push(" \"");
    line.push(cmd_folder(meeting_dir));
    line.push("\"\"");
    Invocation {
        program: "cmd",
        args: vec!["/D".into(), "/S".into(), "/C".into()],
        raw_tail: Some(line),
    }
}

/// The folder as it goes on a `cmd` line. `cmd` expands `%NAME%` even inside
/// quotes, so a folder holding a `%` (`x%COMSPEC%`) is passed as a reference
/// to [`super::MEETING_DIR_ENV`] instead: `cmd` expands that once, and never
/// expands the value it put in (TUR-167). Any other folder goes as written,
/// so a command with a stray `%` of its own still works.
fn cmd_folder(meeting_dir: &Path) -> OsString {
    if meeting_dir.as_os_str().to_string_lossy().contains('%') {
        format!("%{}%", super::MEETING_DIR_ENV).into()
    } else {
        meeting_dir.into()
    }
}

/// Whether this build runs hooks the Windows way.
pub fn is_windows() -> bool {
    cfg!(windows)
}

/// The `Command` for `invocation`, stdio and env left to the caller.
pub fn command(invocation: &Invocation) -> Command {
    let mut command = Command::new(invocation.program);
    command.args(&invocation.args);
    if let Some(tail) = &invocation.raw_tail {
        raw_arg(&mut command, tail);
    }
    command
}

#[cfg(windows)]
fn raw_arg(command: &mut Command, tail: &OsString) {
    use std::os::windows::process::CommandExt as _;
    command.raw_arg(tail);
}

#[cfg(not(windows))]
fn raw_arg(command: &mut Command, tail: &OsString) {
    // Never built by `invocation` off Windows; kept whole if it ever is.
    command.arg(tail);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_runs_sh_with_the_folder_as_the_last_argument() {
        let got = invocation(false, "~/bin/x.sh --flag", Path::new("/m/a b"));
        assert_eq!(got.program, "sh");
        assert_eq!(
            got.args,
            ["-c", "~/bin/x.sh --flag \"$@\"", SH_NAME, "/m/a b"]
                .map(OsString::from)
                .to_vec()
        );
        assert_eq!(got.raw_tail, None);
    }

    #[test]
    fn windows_runs_cmd_with_the_folder_quoted_last() {
        let got = invocation(true, "C:\\h\\x.bat", Path::new("C:\\M\\a b"));
        assert_eq!(got.program, "cmd");
        assert_eq!(got.args, ["/D", "/S", "/C"].map(OsString::from).to_vec());
        assert_eq!(
            got.raw_tail,
            Some(OsString::from("\"C:\\h\\x.bat \"C:\\M\\a b\"\""))
        );
    }

    #[test]
    fn windows_never_puts_a_folder_holding_a_percent_on_the_cmd_line() {
        let got = invocation(true, "x.bat", Path::new("C:\\M\\x%COMSPEC%"));
        assert_eq!(
            got.raw_tail,
            Some(OsString::from("\"x.bat \"%MEETAI_MEETING_DIR%\"\""))
        );
        // PowerShell `-File` arguments are never expanded: the folder as is.
        let got = invocation(true, "x.ps1", Path::new("C:\\M\\x%COMSPEC%"));
        assert_eq!(got.args.last(), Some(&OsString::from("C:\\M\\x%COMSPEC%")));
    }

    #[test]
    fn windows_runs_a_ps1_file_with_powershell() {
        let got = invocation(true, "\"C:\\h\\My X.PS1\" ", Path::new("C:\\M"));
        assert_eq!(got.program, "powershell");
        assert_eq!(got.args.last(), Some(&OsString::from("C:\\M")));
        assert!(got.args.contains(&OsString::from("C:\\h\\My X.PS1")));
        assert!(got.args.contains(&OsString::from("-NoProfile")));
    }
}

/// TUR-167 on a real `cmd`: a folder holding `%NAME%` reaches the hook as
/// written. Only Windows has `cmd`; CI's Windows job runs it.
#[cfg(all(test, windows))]
mod windows_tests {
    use std::time::Duration;

    #[test]
    fn a_folder_holding_a_percent_reaches_the_hook_unexpanded() {
        let dir = tempfile::tempdir().unwrap();
        let meeting = dir.path().join("x%COMSPEC%y");
        std::fs::create_dir(&meeting).unwrap();
        let report = super::super::run_hook("echo arg=", &meeting, Duration::from_secs(20));
        assert!(report.outcome.is_ok(), "{report:?}");
        let shown = meeting.to_string_lossy().into_owned();
        assert!(report.stdout.contains(&shown), "{report:?}");
        assert!(!report.stdout.contains("cmd.exe"), "{report:?}");
    }
}
