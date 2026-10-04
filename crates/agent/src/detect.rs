//! Finding an installed agent CLI and whether it is signed in (SPEC A11,
//! setup screens).
//!
//! An app opened from Finder does not get the shell's `PATH`, so a plain
//! `claude` lookup fails even when it works in Terminal. The search goes, first
//! hit wins:
//!
//! 1. `agent.binary_path` from the config. When it is set, nothing else is
//!    tried: a wrong path shows up as "not found", not as some other copy.
//! 2. The login shell: `$SHELL -lc 'command -v claude'` (macOS and Linux;
//!    Windows has none to ask).
//! 3. The app's own `PATH`, through the `which` crate, which respects
//!    `PATHEXT` on Windows so `claude` finds `claude.cmd` or `claude.exe`.
//! 4. Folders installers use (per OS, `platform::search_dirs`): on macOS
//!    `~/.local/bin` and Homebrew; on Linux also npm's `~/.npm-global/bin`,
//!    `~/.cargo/bin` and Linuxbrew; on Windows `%USERPROFILE%\.local\bin`,
//!    `%APPDATA%\npm`, `%LOCALAPPDATA%\npm` and `%LOCALAPPDATA%\Programs`.
//!    Then the CLI's own (`~/.claude/local` for Claude Code). Each name is
//!    tried with the OS's program endings (`.exe`, `.cmd`, `.bat` on Windows).
//! 5. Copies inside app bundles: Codex inside `ChatGPT.app` or `Codex.app`,
//!    Claude Code inside the Claude desktop app's support folder.
//!
//! Then `--version`, and the CLI's own sign-in check: `claude auth status`,
//! `codex login status`. A Claude Code too old for `auth status` gets a tiny
//! notes run instead, with no tools, no MCP servers and no hooks.
//!
//! Setup is never a side effect of a health check: nothing here starts an MCP
//! server, runs a hook, or opens a sign-in browser. Every command runs with an
//! empty stdin, in a fresh empty folder, under a short time limit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::claude::ClaudeHarness;
use crate::platform::{self, is_executable};
use crate::process::{self, CliOutput, cli_command};
use crate::{AgentError, Harness, Install, Job};

/// Time limit for the shell lookup, `--version` and the sign-in checks.
const QUICK_LIMIT: Duration = Duration::from_secs(15);

/// Time limit for the sample run on an old Claude Code. It is a real model
/// call, so it gets longer.
const SAMPLE_RUN_LIMIT: Duration = Duration::from_secs(60);

/// Longest `--version` line kept. Anything longer is not a version.
const MAX_VERSION_CHARS: usize = 100;

/// Model for the sample run: the cheapest the CLI accepts by alias.
const SAMPLE_MODEL: &str = "haiku";

/// Prompt for the sample run. It only has to come back without an auth
/// error.
const SAMPLE_PROMPT: &str = "Set ok to true.";

/// One agent CLI to look for.
#[derive(Debug, Clone, Copy)]
pub struct Cli {
    /// Command name, as typed in Terminal.
    name: &'static str,
    /// Name the user knows, for errors and logs.
    display_name: &'static str,
    /// Install folders only this CLI uses, relative to home. Checked with
    /// [`Lookup::bin_dirs`].
    own_dirs: &'static [&'static str],
    /// Copies inside app bundles, relative to each of [`Lookup::app_dirs`].
    ///
    /// Only `Contents/Resources` paths: APFS is case-insensitive, so a
    /// `Contents/MacOS/codex` would match the app's own `Codex` executable,
    /// and running that with `--version` would open a window.
    bundled: &'static [&'static str],
    /// A copy kept in one folder per version.
    versioned: Option<Versioned>,
    /// The CLI's sign-in check.
    sign_in: fn(&Path, &Lookup) -> bool,
}

/// A folder under home with one subfolder per version (`2.1.284/`), each
/// holding the binary at the same path. The highest version wins.
#[derive(Debug, Clone, Copy)]
struct Versioned {
    /// The folder holding the version subfolders, relative to home.
    root: &'static str,
    /// The binary's path inside each version subfolder.
    binary: &'static str,
}

/// Claude Code. The Claude desktop app keeps its own copy, one folder per
/// version, under `~/Library/Application Support/Claude/claude-code`.
pub const CLAUDE: Cli = Cli {
    name: "claude",
    display_name: "Claude Code",
    own_dirs: &[".claude/local"],
    bundled: &[],
    versioned: Some(Versioned {
        root: "Library/Application Support/Claude/claude-code",
        binary: "claude.app/Contents/MacOS/claude",
    }),
    sign_in: claude_signed_in,
};

/// Codex. ChatGPT.app ships it and does not put it on `PATH` (found
/// 2026-10-01); a standalone Codex.app is checked the same way.
pub const CODEX: Cli = Cli {
    name: "codex",
    display_name: "Codex",
    own_dirs: &[],
    bundled: &[
        "ChatGPT.app/Contents/Resources/codex",
        "Codex.app/Contents/Resources/codex",
    ],
    versioned: None,
    sign_in: codex_signed_in,
};

/// Where to look. [`Lookup::system`] is this Mac; tests point every field at
/// fakes.
#[derive(Debug, Clone)]
pub struct Lookup {
    /// `agent.binary_path`. When set, it is the only place looked at.
    pub binary_path: Option<PathBuf>,
    /// The login shell to ask. `None` on Windows, which has none.
    pub shell: Option<PathBuf>,
    /// The `PATH` to search with `which`. `None`: skip that step.
    pub path_var: Option<OsString>,
    /// Program endings tried after each command name in [`Lookup::bin_dirs`]:
    /// `[""]` on unix, `.exe`, `.cmd`, ... on Windows.
    pub exe_suffixes: &'static [&'static str],
    /// The user's home folder.
    pub home: Option<PathBuf>,
    /// Install folders any CLI may be in, checked after the shell.
    pub bin_dirs: Vec<PathBuf>,
    /// Folders holding `.app` bundles.
    pub app_dirs: Vec<PathBuf>,
    /// Where each command's fresh, empty working folder is made.
    pub work_root: PathBuf,
    /// Time limit for the shell lookup, `--version` and sign-in checks.
    pub quick_limit: Duration,
    /// Time limit for the sample run.
    pub sample_run_limit: Duration,
}

impl Lookup {
    /// This computer: its login shell (none on Windows), the app's `PATH`,
    /// the home folder, the OS's install and app folders
    /// (`platform::search_dirs`), and the system temp folder.
    pub fn system(binary_path: Option<&Path>) -> Self {
        let home = dirs::home_dir().filter(|p| p.is_absolute());
        let (bin_dirs, app_dirs) = platform::search_dirs(home.as_deref());
        Self {
            binary_path: binary_path.map(Path::to_path_buf),
            shell: platform::login_shell(),
            path_var: std::env::var_os("PATH"),
            exe_suffixes: platform::EXE_SUFFIXES,
            home,
            bin_dirs,
            app_dirs,
            work_root: std::env::temp_dir(),
            quick_limit: QUICK_LIMIT,
            sample_run_limit: SAMPLE_RUN_LIMIT,
        }
    }
}

/// Finds Claude Code on this computer.
///
/// `binary_path` is `agent.binary_path`. That one config key belongs to the
/// harness the user picked, so pass it only to that harness's detect, and
/// `None` to the other one; otherwise both report the same binary.
pub fn claude(binary_path: Option<&Path>) -> Option<Install> {
    detect(&CLAUDE, &Lookup::system(binary_path))
}

/// Finds Codex on this computer.
///
/// `binary_path` is `agent.binary_path`: pass it only when Codex is the
/// harness the user picked, as for [`claude`].
pub fn codex(binary_path: Option<&Path>) -> Option<Install> {
    detect(&CODEX, &Lookup::system(binary_path))
}

/// Finds `cli`, then asks it for its version and sign-in state. `None` when
/// it is not installed.
pub fn detect(cli: &Cli, lookup: &Lookup) -> Option<Install> {
    let path = find(cli, lookup)?;
    let version = version(cli, &path, lookup);
    let signed_in = (cli.sign_in)(&path, lookup);
    tracing::debug!(cli = cli.name, path = %path.display(), ?version, signed_in, "agent CLI found");
    Some(Install {
        path,
        version,
        signed_in,
    })
}

/// Where `cli` is, in the order the module docs give.
// Adapted from github.com/silverstein/minutes/crates/core/src/summarize.rs @ c1e236acf3a3aea0729976cfc6959dcceb5cd75f (MIT)
// (`resolve_agent_path`: absolute path, then `which`, then known folders
// with each program ending). Copyright (c) 2026 Mat Silverstein.
pub fn find(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    if let Some(configured) = &lookup.binary_path {
        // On Windows a configured `...\claude` may mean `claude.cmd`; only
        // endings of that one path are tried, never another folder.
        let found = expand_home(configured, lookup.home.as_deref())
            .and_then(|path| with_suffixes(path, lookup.exe_suffixes).find(|p| is_executable(p)));
        if found.is_some() {
            return found;
        }
        tracing::warn!(cli = cli.name, path = %configured.display(), "agent.binary_path is not an executable file");
        return None;
    }
    let own_dirs = lookup
        .home
        .iter()
        .flat_map(|home| cli.own_dirs.iter().map(move |rel| home.join(rel)));
    from_login_shell(cli, lookup)
        .or_else(|| from_path_var(cli, lookup))
        .or_else(|| {
            lookup
                .bin_dirs
                .iter()
                .cloned()
                .chain(own_dirs)
                .flat_map(|dir| with_suffixes(dir.join(cli.name), lookup.exe_suffixes))
                .find(|p| is_executable(p))
        })
        .or_else(|| {
            lookup
                .app_dirs
                .iter()
                .flat_map(|dir| cli.bundled.iter().map(move |rel| dir.join(rel)))
                .find(|p| is_executable(p))
        })
        .or_else(|| newest_versioned(cli, lookup))
}

/// `path` itself, then `path` with each of `suffixes` added to its name.
fn with_suffixes(path: PathBuf, suffixes: &[&str]) -> impl Iterator<Item = PathBuf> {
    let extra: Vec<PathBuf> = suffixes
        .iter()
        .filter(|s| !s.is_empty())
        .map(|suffix| {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            PathBuf::from(name)
        })
        .collect();
    std::iter::once(path).chain(extra)
}

/// `cli.name` on [`Lookup::path_var`], through `which` (`PATHEXT` on
/// Windows).
fn from_path_var(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    let path_var = lookup.path_var.as_ref()?;
    which::which_in(cli.name, Some(path_var), &lookup.work_root)
        .ok()
        .filter(|p| p.is_absolute() && is_executable(p))
}

/// `$SHELL -lc 'command -v <name>'`. The shell's startup files may print
/// other lines, so the last absolute path that is executable wins.
fn from_login_shell(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    let mut command = Command::new(lookup.shell.as_ref()?);
    // `cli.name` is one of this module's constants, never user input.
    command.arg("-lc").arg(format!("command -v {}", cli.name));
    let out = probe(lookup, "login shell", command)
        .inspect_err(|e| match e {
            // Its stderr can hold anything the user's startup files print.
            AgentError::CliFailed { status, .. } => {
                tracing::debug!(cli = cli.name, ?status, "login shell lookup failed");
            }
            e => tracing::debug!(cli = cli.name, "login shell lookup failed: {e}"),
        })
        .ok()?;
    out.stdout
        .lines()
        .rev()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|p| p.is_absolute() && is_executable(p))
}

/// The highest-numbered copy in `cli.versioned`, if there is one.
fn newest_versioned(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    let versioned = cli.versioned?;
    let entries = std::fs::read_dir(lookup.home.as_ref()?.join(versioned.root)).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let number = version_number(&entry.file_name().to_string_lossy())?;
            let path = entry.path().join(versioned.binary);
            is_executable(&path).then_some((number, path))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

/// `"2.1.286"` as `[2, 1, 286]`. `None` for anything that is not dotted
/// numbers.
fn version_number(name: &str) -> Option<Vec<u64>> {
    name.split('.').map(|part| part.parse().ok()).collect()
}

/// The first line `--version` printed, if it printed one.
fn version(cli: &Cli, path: &Path, lookup: &Lookup) -> Option<String> {
    let mut command = cli_command(path);
    command.arg("--version");
    let out = probe(lookup, cli.display_name, command)
        .inspect_err(|e| tracing::debug!(cli = cli.name, "--version failed: {e}"))
        .ok()?;
    first_line(&out.stdout).or_else(|| first_line(&out.stderr))
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .filter(|line| line.chars().count() <= MAX_VERSION_CHARS)
        .map(str::to_owned)
}

/// `claude auth status` prints JSON with `loggedIn`, and exits non-zero when
/// nobody is signed in. A Claude Code whose `auth --help` works but lists no
/// `status` command gets the sample run instead.
///
/// `auth --help` is asked first because an old CLI with no `auth` command
/// would take `auth status` as a prompt. `--help` is safe on any version. If
/// `auth --help` itself fails, the answer is "signed out": the sample run is
/// a real model call, too costly to make on a guess.
fn claude_signed_in(path: &Path, lookup: &Lookup) -> bool {
    let mut help = cli_command(path);
    help.args(["auth", "--help"]);
    let help = match probe(lookup, CLAUDE.display_name, help) {
        Ok(out) => out.stdout,
        Err(e) => {
            tracing::debug!("claude auth --help: {e}");
            return false;
        }
    };
    if !lists_status_command(&help) {
        return claude_sample_run(path, lookup);
    }
    let mut command = cli_command(path);
    command.args(["auth", "status"]);
    match probe(lookup, CLAUDE.display_name, command) {
        Ok(out) => logged_in_json(&out.stdout),
        Err(e) => {
            tracing::debug!("claude auth status: {e}");
            false
        }
    }
}

/// Whether help text lists a `status` command: a line whose first word is
/// `status`, as in `  status [options]  Show authentication status`.
fn lists_status_command(help: &str) -> bool {
    help.lines()
        .any(|line| line.split_whitespace().next() == Some("status"))
}

/// Reads only `loggedIn`: the rest of the reply names the account, and none
/// of it is kept or logged.
fn logged_in_json(stdout: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v.get("loggedIn")?.as_bool())
        .unwrap_or(false)
}

/// A tiny notes run through [`ClaudeHarness`] itself, so it has exactly the
/// notes run's flags (SPEC A11): no tools, no MCP servers, no hooks. Signed
/// in when it comes back with a reply.
///
/// A CLI too old to know those flags fails here and reads as signed out,
/// which is right: it could not run the notes either.
fn claude_sample_run(path: &Path, lookup: &Lookup) -> bool {
    let mut harness = ClaudeHarness::new().with_binary(path);
    if let Some(search_path) = path.parent().and_then(process::search_path_with) {
        harness = harness.with_search_path(search_path);
    }
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"],
        "additionalProperties": false,
    });
    let mut job = Job::notes(SAMPLE_PROMPT, schema);
    job.model = Some(SAMPLE_MODEL.to_owned());
    job.timeout = lookup.sample_run_limit;
    job.work_root = lookup.work_root.clone();
    harness
        .run(&job)
        .inspect_err(|e| tracing::debug!("claude sign-in run: {e}"))
        .is_ok()
}

/// `codex login status` prints "Logged in using ChatGPT" (on stderr) and
/// exits 0 when signed in.
fn codex_signed_in(path: &Path, lookup: &Lookup) -> bool {
    let mut command = cli_command(path);
    command.args(["login", "status"]);
    match probe(lookup, CODEX.display_name, command) {
        Ok(out) => [&out.stdout, &out.stderr].iter().any(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("Logged in"))
        }),
        Err(e) => {
            tracing::debug!("codex login status: {e}");
            false
        }
    }
}

/// [`process::run_probe`] with this lookup's work root and quick limit.
fn probe(lookup: &Lookup, display_name: &str, command: Command) -> Result<CliOutput, AgentError> {
    process::run_probe(display_name, command, lookup.quick_limit, &lookup.work_root)
}

/// `~/x` as `<home>/x`. Anything still relative is refused.
fn expand_home(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let path = match path.strip_prefix("~") {
        Ok(rest) => home?.join(rest),
        Err(_) => path.to_path_buf(),
    };
    path.is_absolute().then_some(path)
}

/// The search order on every OS, on a fake folder layout. Nothing is run:
/// only `find`, which looks at files.
#[cfg(test)]
mod find_tests {
    use super::*;
    use std::fs;

    /// A fake home with this OS's folders under it, and an empty work root.
    struct Fake {
        dir: tempfile::TempDir,
    }

    impl Fake {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("home")).unwrap();
            fs::create_dir_all(dir.path().join("work")).unwrap();
            Self { dir }
        }

        fn home(&self) -> PathBuf {
            self.dir.path().join("home")
        }

        /// This OS's lookup on the fake home: no shell, no `PATH`.
        fn lookup(&self) -> Lookup {
            let (bin_dirs, app_dirs) = platform::search_dirs_under(&self.home());
            Lookup {
                binary_path: None,
                shell: None,
                path_var: None,
                exe_suffixes: platform::EXE_SUFFIXES,
                home: Some(self.home()),
                bin_dirs: bin_dirs
                    .into_iter()
                    .filter(|d| d.starts_with(self.dir.path()))
                    .collect(),
                app_dirs: app_dirs
                    .into_iter()
                    .filter(|d| d.starts_with(self.dir.path()))
                    .collect(),
                work_root: self.dir.path().join("work"),
                quick_limit: Duration::from_secs(5),
                sample_run_limit: Duration::from_secs(5),
            }
        }

        /// A runnable file at `dir/<name><suffix>`.
        fn program(&self, dir: &Path, name: &str, suffix: &str) -> PathBuf {
            fs::create_dir_all(dir).unwrap();
            let path = dir.join(format!("{name}{suffix}"));
            fs::write(&path, "").unwrap();
            platform::make_executable(&path);
            path
        }
    }

    #[test]
    fn each_install_folder_and_program_ending_is_found() {
        let dirs = Fake::new().lookup().bin_dirs;
        assert!(!dirs.is_empty());
        for (i, _) in dirs.iter().enumerate() {
            for suffix in platform::EXE_SUFFIXES {
                let fake = Fake::new();
                let lookup = fake.lookup();
                let dir = &lookup.bin_dirs[i];
                let claude = fake.program(dir, "claude", suffix);
                let codex = fake.program(dir, "codex", suffix);
                assert_eq!(find(&CLAUDE, &lookup), Some(claude), "{dir:?} {suffix:?}");
                assert_eq!(find(&CODEX, &lookup), Some(codex), "{dir:?} {suffix:?}");
            }
        }
    }

    #[test]
    fn earlier_folders_win() {
        let fake = Fake::new();
        let lookup = fake.lookup();
        let suffix = platform::EXE_SUFFIXES[0];
        let last = fake.program(lookup.bin_dirs.last().unwrap(), "claude", suffix);
        assert_eq!(find(&CLAUDE, &lookup), Some(last));
        let first = fake.program(&lookup.bin_dirs[0], "claude", suffix);
        assert_eq!(find(&CLAUDE, &lookup), Some(first));
    }

    #[test]
    fn path_is_searched_before_install_folders() {
        let fake = Fake::new();
        let mut lookup = fake.lookup();
        // The ending `which` adds itself (`PATHEXT` on Windows).
        let suffix = platform::EXE_SUFFIXES[platform::EXE_SUFFIXES.len().min(2) - 1];
        fake.program(&lookup.bin_dirs[0], "claude", suffix);
        let on_path = fake.program(&fake.dir.path().join("on-path"), "claude", suffix);
        lookup.path_var = Some(fake.dir.path().join("on-path").into_os_string());
        let found = find(&CLAUDE, &lookup).unwrap();
        assert!(
            found
                .to_string_lossy()
                .eq_ignore_ascii_case(&on_path.to_string_lossy()),
            "{found:?}"
        );
    }

    #[test]
    fn binary_path_wins_and_may_leave_off_the_ending() {
        let fake = Fake::new();
        let mut lookup = fake.lookup();
        let suffix = *platform::EXE_SUFFIXES.last().unwrap();
        fake.program(&lookup.bin_dirs[0], "claude", suffix);
        let custom = fake.program(&fake.dir.path().join("custom"), "claude", suffix);
        lookup.binary_path = Some(fake.dir.path().join("custom").join("claude"));
        assert_eq!(find(&CLAUDE, &lookup), Some(custom));

        lookup.binary_path = Some(fake.dir.path().join("nowhere").join("claude"));
        assert_eq!(find(&CLAUDE, &lookup), None);
    }

    #[test]
    fn a_non_program_file_is_skipped() {
        let fake = Fake::new();
        let lookup = fake.lookup();
        let path = lookup.bin_dirs[0].join("claude.txt");
        fs::create_dir_all(&lookup.bin_dirs[0]).unwrap();
        fs::write(&path, "").unwrap();
        assert_eq!(find(&CLAUDE, &lookup), None);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use test_support::FakeCli;

    /// A fake Mac in a temp folder: a home, an app folder, a bin folder, and
    /// a working root. Its "CLIs" are copies of `fake-cli` (test-support), so
    /// these tests run on every OS.
    pub(crate) struct FakeMac {
        dir: tempfile::TempDir,
    }

    impl FakeMac {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            for sub in ["home", "apps", "bin", "work"] {
                fs::create_dir(dir.path().join(sub)).unwrap();
            }
            Self { dir }
        }

        pub(crate) fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        /// A lookup with no shell hit, no binary_path, and short limits.
        pub(crate) fn lookup(&self) -> Lookup {
            Lookup {
                binary_path: None,
                shell: Some(self.cli("shell", "*\t1\t\t")),
                path_var: None,
                exe_suffixes: platform::EXE_SUFFIXES,
                home: Some(self.path("home")),
                bin_dirs: vec![self.path("bin")],
                app_dirs: vec![self.path("apps")],
                work_root: self.path("work"),
                quick_limit: Duration::from_secs(5),
                sample_run_limit: Duration::from_secs(5),
            }
        }

        /// A fake CLI at `rel` (plus `.exe` on Windows) that answers with
        /// `cases` (see `fake-cli`'s header) and logs every call's arguments
        /// to `<rel>.log`.
        pub(crate) fn cli(&self, rel: &str, cases: &str) -> PathBuf {
            let at = self.path(rel);
            let cli = FakeCli::install(at.parent().unwrap(), &file_name(&at));
            cli.set("cases", cases)
                .set("log_append", at.with_extension("log").display().to_string());
            cli.path().to_path_buf()
        }

        /// Gives a CLI a reply delay, for the time-limit tests.
        pub(crate) fn sleep(&self, cli: &Path, secs: &str) {
            let stem = cli.file_stem().unwrap().to_string_lossy().into_owned();
            fs::write(
                cli.with_file_name(format!("{stem}.fake")).join("sleep"),
                secs,
            )
            .unwrap();
        }

        pub(crate) fn log(&self, rel: &str) -> String {
            fs::read_to_string(self.path(rel).with_extension("log")).unwrap_or_default()
        }
    }

    fn file_name(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    /// A Claude Code new enough for `auth status`, signed in.
    pub(crate) const CLAUDE_NEW: &str = "--version\t0\t2.1.286 (Claude Code)\\n\t
auth --help\t0\tCommands:\\n  login [options]  Sign in\\n  status [options]  Show authentication status\\n\t
auth status\t0\t{\"loggedIn\": true, \"email\": \"a@b.c\"}\\n\t";

    #[test]
    fn login_shell_is_asked_and_its_noise_skipped() {
        let mac = FakeMac::new();
        let claude = mac.cli("opt/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.shell = Some(mac.cli(
            "shell",
            &format!(
                "-lc command -v claude\t0\tWelcome from .zprofile\\n{}\\n\t\n*\t9\t\t",
                claude.display()
            ),
        ));

        let install = detect(&CLAUDE, &lookup).unwrap();
        assert_eq!(install.path, claude);
        assert_eq!(install.version.as_deref(), Some("2.1.286 (Claude Code)"));
        assert!(install.signed_in);
        assert!(!mac.log("opt/claude").contains("-p"), "no model run needed");
    }

    #[test]
    fn a_shell_hit_that_is_not_executable_is_skipped() {
        let mac = FakeMac::new();
        let plain = mac.path("opt/claude");
        fs::create_dir_all(plain.parent().unwrap()).unwrap();
        fs::write(&plain, "not a program").unwrap();
        let mut lookup = mac.lookup();
        lookup.shell = Some(mac.cli("shell", &format!("*\t0\t{}\\n\t", plain.display())));

        assert_eq!(find(&CLAUDE, &lookup), None);
    }

    #[test]
    fn a_hanging_login_shell_is_cut_off() {
        let mac = FakeMac::new();
        let mut lookup = mac.lookup();
        let shell = mac.cli("shell", "*\t0\t\t");
        mac.sleep(&shell, "30");
        lookup.shell = Some(shell);
        lookup.quick_limit = Duration::from_millis(300);
        let in_bin = mac.cli("bin/claude", CLAUDE_NEW);

        let started = std::time::Instant::now();
        assert_eq!(find(&CLAUDE, &lookup), Some(in_bin));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn binary_path_wins_over_everything() {
        let mac = FakeMac::new();
        let configured = mac.cli("custom/claude", CLAUDE_NEW);
        mac.cli("bin/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.shell = Some(mac.cli("shell", "*\t1\tshould-not-be-asked\\n\t"));
        lookup.binary_path = Some(mac.path("custom/claude"));

        assert_eq!(find(&CLAUDE, &lookup), Some(configured));
        assert_eq!(mac.log("shell"), "", "the shell was asked");
    }

    #[test]
    fn binary_path_with_a_tilde_is_expanded() {
        let mac = FakeMac::new();
        let configured = mac.cli("home/tools/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.binary_path = Some("~/tools/claude".into());

        assert_eq!(find(&CLAUDE, &lookup), Some(configured));
    }

    #[test]
    fn a_wrong_binary_path_is_not_found_rather_than_replaced() {
        let mac = FakeMac::new();
        mac.cli("bin/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.binary_path = Some(mac.path("nowhere/claude"));
        assert_eq!(find(&CLAUDE, &lookup), None);

        lookup.binary_path = Some("relative/claude".into());
        assert_eq!(find(&CLAUDE, &lookup), None);
    }

    #[test]
    fn nothing_anywhere_is_not_installed() {
        let mac = FakeMac::new();
        assert_eq!(detect(&CLAUDE, &mac.lookup()), None);
        assert_eq!(detect(&CODEX, &mac.lookup()), None);
    }

    #[test]
    fn codex_signed_in_is_reported() {
        let mac = FakeMac::new();
        let codex = mac.cli(
            "bin/codex",
            "--version\t0\tcodex-cli 0.50.0\\n\t\nlogin status\t0\t\tLogged in using ChatGPT\\n",
        );

        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert_eq!(install.path, codex);
        assert_eq!(install.version.as_deref(), Some("codex-cli 0.50.0"));
        assert!(install.signed_in);
    }

    #[test]
    fn codex_signed_out_is_reported() {
        let mac = FakeMac::new();
        mac.cli(
            "bin/codex",
            "--version\t0\tcodex-cli 0.50.0\\n\t\nlogin status\t1\t\tNot logged in\\n",
        );

        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert!(!install.signed_in);
    }

    #[test]
    fn claude_signed_out_is_reported() {
        let mac = FakeMac::new();
        mac.cli(
            "bin/claude",
            "--version\t0\t2.1.286 (Claude Code)\\n\t
auth --help\t0\tCommands:\\n  login [options]  Sign in\\n  status [options]  Show authentication status\\n\t
auth status\t1\t{\"loggedIn\": false}\\n\t",
        );

        let install = detect(&CLAUDE, &mac.lookup()).unwrap();
        assert!(!install.signed_in);
        assert!(!mac.log("bin/claude").contains("-p"));
    }

    #[test]
    fn old_claude_gets_a_no_tools_sample_run() {
        let mac = FakeMac::new();
        mac.cli(
            "bin/claude",
            "--version\t0\t1.0.0 (Claude Code)\\n\t
auth\t0\tUsage: claude [options] [prompt]\\n  Check your status with /status\\n\t
-p\t0\t{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"structured_output\":{\"ok\":true}}\\n\t",
        );

        let install = detect(&CLAUDE, &mac.lookup()).unwrap();
        assert!(install.signed_in);
        let log = mac.log("bin/claude");
        assert!(!log.contains("auth status"), "{log}");
        let run = log.lines().find(|l| l.starts_with("-p")).unwrap();
        for flag in [
            "--tools  ",
            "--strict-mcp-config",
            "--permission-mode dontAsk",
            r#""disableAllHooks":true"#,
        ] {
            assert!(run.contains(flag), "{flag} missing from {run}");
        }
    }

    #[test]
    fn old_claude_with_an_auth_error_is_signed_out() {
        let mac = FakeMac::new();
        mac.cli(
            "bin/claude",
            "--version\t0\t1.0.0 (Claude Code)\\n\t
-p\t1\t{\"type\":\"result\",\"is_error\":true,\"result\":\"Invalid API key\"}\\n\t
*\t0\t\t",
        );

        assert!(!detect(&CLAUDE, &mac.lookup()).unwrap().signed_in);
    }

    #[test]
    fn a_failing_auth_help_is_signed_out_without_a_model_run() {
        let mac = FakeMac::new();
        mac.cli(
            "bin/claude",
            "--version\t0\t2.1.286 (Claude Code)\\n\t
auth --help\t1\t\t
*\t0\t{\"type\":\"result\",\"is_error\":false,\"structured_output\":{\"ok\":true}}\\n\t",
        );

        assert!(!detect(&CLAUDE, &mac.lookup()).unwrap().signed_in);
        let log = mac.log("bin/claude");
        assert!(!log.contains("-p"), "{log}");
        assert!(!log.contains("auth status"), "{log}");
    }

    #[test]
    fn a_cli_only_folder_is_searched_for_that_cli_only() {
        let mac = FakeMac::new();
        let claude = mac.cli("home/.claude/local/claude", CLAUDE_NEW);
        mac.cli("home/.claude/local/codex", "*\t0\t\t");

        assert_eq!(find(&CLAUDE, &mac.lookup()), Some(claude));
        assert_eq!(find(&CODEX, &mac.lookup()), None);
    }

    #[test]
    fn only_a_status_command_line_counts() {
        assert!(lists_status_command(
            "Commands:\n  login  Sign in\n  status [options]  Show status\n"
        ));
        assert!(!lists_status_command(
            "Usage: claude [prompt]\n  Check your status with /status\n"
        ));
        assert!(!lists_status_command(""));
    }

    #[test]
    fn a_version_that_prints_nothing_is_none() {
        let mac = FakeMac::new();
        mac.cli("bin/codex", "*\t0\t\t");
        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert_eq!(install.version, None);
        assert!(!install.signed_in);
    }

    #[test]
    fn cli_command_puts_the_cli_folder_first_on_path() {
        let command = cli_command(Path::new("/opt/tools/bin/claude"));
        let path = command
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v)
            .unwrap();
        assert!(path.to_string_lossy().starts_with("/opt/tools/bin"));
    }

    #[test]
    fn version_numbers_compare_as_numbers() {
        assert!(version_number("2.1.284") > version_number("2.1.9"));
        assert_eq!(version_number("latest"), None);
    }

    #[test]
    fn logged_in_reads_only_the_flag() {
        assert!(logged_in_json(r#"{"loggedIn": true}"#));
        assert!(!logged_in_json(r#"{"loggedIn": false}"#));
        assert!(!logged_in_json("not json"));
        assert!(!logged_in_json(r#"{"loggedIn": "yes"}"#));
    }

    /// The real CLIs on this Mac. Run by hand: `cargo test -p agent --
    /// --ignored real_`.
    #[test]
    #[ignore = "needs the real claude and codex CLIs, signed in"]
    fn real_clis_are_found() {
        let claude = claude(None).expect("claude not found");
        assert!(claude.signed_in, "{claude:?}");
        let codex = codex(None).expect("codex not found");
        assert!(codex.signed_in, "{codex:?}");
    }
}
