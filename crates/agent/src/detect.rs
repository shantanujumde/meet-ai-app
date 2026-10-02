//! Finding an installed agent CLI and whether it is signed in (SPEC A11,
//! setup screens).
//!
//! An app opened from Finder does not get the shell's `PATH`, so a plain
//! `claude` lookup fails even when it works in Terminal. The search goes, first
//! hit wins:
//!
//! 1. `agent.binary_path` from the config. When it is set, nothing else is
//!    tried: a wrong path shows up as "not found", not as some other copy.
//! 2. The login shell: `$SHELL -lc 'command -v claude'`.
//! 3. Folders installers use: `~/.local/bin`, Homebrew, and the CLI's own
//!    (`~/.claude/local` for Claude Code).
//! 4. Copies inside app bundles: Codex inside `ChatGPT.app` or `Codex.app`,
//!    Claude Code inside the Claude desktop app's support folder.
//!
//! Then `--version`, and the CLI's own sign-in check: `claude auth status`,
//! `codex login status`. A Claude Code too old for `auth status` gets a tiny
//! notes run instead, with no tools, no MCP servers and no hooks.
//!
//! Setup is never a side effect of a health check: nothing here starts an MCP
//! server, runs a hook, or opens a sign-in browser. Every command runs with an
//! empty stdin, in a fresh empty folder, under a short time limit.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::claude::ClaudeHarness;
use crate::process::{self, CliOutput, cli_command};
use crate::{AgentError, Harness, Install, Job};

/// Time limit for the shell lookup, `--version` and the sign-in checks.
const QUICK_LIMIT: Duration = Duration::from_secs(15);

/// Time limit for the sample run on an old Claude Code. It is a real model
/// call, so it gets longer.
const SAMPLE_RUN_LIMIT: Duration = Duration::from_secs(60);

/// Longest `--version` line kept. Anything longer is not a version.
const MAX_VERSION_CHARS: usize = 100;

/// The shell used when `$SHELL` is unset or not absolute: the macOS default.
const DEFAULT_SHELL: &str = "/bin/zsh";

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
    /// The login shell to ask.
    pub shell: PathBuf,
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
    /// This Mac: `$SHELL`, the home folder, `/Applications` and
    /// `~/Applications`, and the system temp folder.
    pub fn system(binary_path: Option<&Path>) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SHELL));
        let mut bin_dirs = Vec::new();
        let mut app_dirs = vec![PathBuf::from("/Applications")];
        if let Some(home) = &home {
            bin_dirs.push(home.join(".local/bin"));
            app_dirs.push(home.join("Applications"));
        }
        bin_dirs.push("/opt/homebrew/bin".into());
        bin_dirs.push("/usr/local/bin".into());
        Self {
            binary_path: binary_path.map(Path::to_path_buf),
            shell,
            home,
            bin_dirs,
            app_dirs,
            work_root: std::env::temp_dir(),
            quick_limit: QUICK_LIMIT,
            sample_run_limit: SAMPLE_RUN_LIMIT,
        }
    }
}

/// Finds Claude Code on this Mac.
///
/// `binary_path` is `agent.binary_path`. That one config key belongs to the
/// harness the user picked, so pass it only to that harness's detect, and
/// `None` to the other one; otherwise both report the same binary.
pub fn claude(binary_path: Option<&Path>) -> Option<Install> {
    detect(&CLAUDE, &Lookup::system(binary_path))
}

/// Finds Codex on this Mac.
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
pub fn find(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    if let Some(configured) = &lookup.binary_path {
        let path = expand_home(configured, lookup.home.as_deref());
        if path.as_deref().is_some_and(is_executable) {
            return path;
        }
        tracing::warn!(cli = cli.name, path = %configured.display(), "agent.binary_path is not an executable file");
        return None;
    }
    let own_dirs = lookup
        .home
        .iter()
        .flat_map(|home| cli.own_dirs.iter().map(move |rel| home.join(rel)));
    from_login_shell(cli, lookup)
        .or_else(|| {
            lookup
                .bin_dirs
                .iter()
                .cloned()
                .chain(own_dirs)
                .map(|dir| dir.join(cli.name))
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

/// `$SHELL -lc 'command -v <name>'`. The shell's startup files may print
/// other lines, so the last absolute path that is executable wins.
fn from_login_shell(cli: &Cli, lookup: &Lookup) -> Option<PathBuf> {
    let mut command = Command::new(&lookup.shell);
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

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// A fake Mac in a temp folder: a home, an app folder, a bin folder, and
    /// a working root.
    struct FakeMac {
        dir: tempfile::TempDir,
    }

    impl FakeMac {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            for sub in ["home", "apps", "bin", "work"] {
                fs::create_dir(dir.path().join(sub)).unwrap();
            }
            Self { dir }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        /// A lookup with no shell hit, no binary_path, and short limits.
        fn lookup(&self) -> Lookup {
            Lookup {
                binary_path: None,
                shell: self.script("shell", "exit 1"),
                home: Some(self.path("home")),
                bin_dirs: vec![self.path("bin")],
                app_dirs: vec![self.path("apps")],
                work_root: self.path("work"),
                quick_limit: Duration::from_secs(5),
                sample_run_limit: Duration::from_secs(5),
            }
        }

        /// Writes an executable `/bin/sh` script at `rel`.
        fn script(&self, rel: &str, body: &str) -> PathBuf {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// A fake `claude` that logs its arguments to `<path>.log`.
        fn fake_claude(&self, rel: &str, body: &str) -> PathBuf {
            let log = self.path(rel).with_extension("log");
            self.script(
                rel,
                &format!("printf '%s\\n' \"$*\" >> '{}'\n{body}", log.display()),
            )
        }

        fn log(&self, rel: &str) -> String {
            fs::read_to_string(self.path(rel).with_extension("log")).unwrap_or_default()
        }
    }

    const CLAUDE_NEW: &str = r#"case "$1 $2" in
  "--version ") echo "2.1.286 (Claude Code)" ;;
  "auth --help") printf 'Commands:\n  login [options]  Sign in\n  status [options]  Show authentication status\n' ;;
  "auth status") echo '{"loggedIn": true, "email": "a@b.c"}' ;;
  *) exit 7 ;;
esac"#;

    #[test]
    fn login_shell_is_asked_and_its_noise_skipped() {
        let mac = FakeMac::new();
        let claude = mac.fake_claude("opt/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.shell = mac.script(
            "shell",
            &format!(
                r#"[ "$1" = "-lc" ] && [ "$2" = "command -v claude" ] || exit 9
echo "Welcome from .zprofile"
echo "{}""#,
                claude.display()
            ),
        );

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
        lookup.shell = mac.script("shell", &format!("echo '{}'", plain.display()));

        assert_eq!(find(&CLAUDE, &lookup), None);
    }

    #[test]
    fn a_hanging_login_shell_is_cut_off() {
        let mac = FakeMac::new();
        let mut lookup = mac.lookup();
        lookup.shell = mac.script("shell", "sleep 30 </dev/null >/dev/null 2>&1");
        lookup.quick_limit = Duration::from_millis(300);
        let in_bin = mac.fake_claude("bin/claude", CLAUDE_NEW);

        let started = std::time::Instant::now();
        assert_eq!(find(&CLAUDE, &lookup), Some(in_bin));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn binary_path_wins_over_everything() {
        let mac = FakeMac::new();
        let configured = mac.fake_claude("custom/claude", CLAUDE_NEW);
        mac.fake_claude("bin/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.shell = mac.script("shell", "echo should-not-be-asked; exit 1");
        lookup.binary_path = Some(configured.clone());

        assert_eq!(find(&CLAUDE, &lookup), Some(configured));
    }

    #[test]
    fn binary_path_with_a_tilde_is_expanded() {
        let mac = FakeMac::new();
        let configured = mac.fake_claude("home/tools/claude", CLAUDE_NEW);
        let mut lookup = mac.lookup();
        lookup.binary_path = Some("~/tools/claude".into());

        assert_eq!(find(&CLAUDE, &lookup), Some(configured));
    }

    #[test]
    fn a_wrong_binary_path_is_not_found_rather_than_replaced() {
        let mac = FakeMac::new();
        mac.fake_claude("bin/claude", CLAUDE_NEW);
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
    fn codex_is_found_inside_chatgpt_app() {
        let mac = FakeMac::new();
        let codex = mac.script(
            "apps/ChatGPT.app/Contents/Resources/codex",
            r#"case "$1 $2" in
  "--version ") echo "codex-cli 0.50.0" ;;
  "login status") echo "Logged in using ChatGPT" >&2 ;;
  *) exit 7 ;;
esac"#,
        );

        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert_eq!(install.path, codex);
        assert_eq!(install.version.as_deref(), Some("codex-cli 0.50.0"));
        assert!(install.signed_in);
    }

    #[test]
    fn codex_is_found_inside_a_standalone_codex_app() {
        let mac = FakeMac::new();
        let codex = mac.script("apps/Codex.app/Contents/Resources/codex", "exit 0");
        assert_eq!(find(&CODEX, &mac.lookup()), Some(codex));
    }

    #[test]
    fn codex_signed_out_is_reported() {
        let mac = FakeMac::new();
        mac.script(
            "bin/codex",
            r#"case "$1 $2" in
  "--version ") echo "codex-cli 0.50.0" ;;
  "login status") echo "Not logged in" >&2; exit 1 ;;
esac"#,
        );

        let install = detect(&CODEX, &mac.lookup()).unwrap();
        assert!(!install.signed_in);
    }

    #[test]
    fn claude_desktop_copy_is_found_and_the_newest_wins() {
        let mac = FakeMac::new();
        let root = "home/Library/Application Support/Claude/claude-code";
        for version in ["2.1.9", "2.1.284", "not-a-version"] {
            mac.fake_claude(
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
        let in_bin = mac.fake_claude("bin/claude", CLAUDE_NEW);
        mac.fake_claude(
            "home/Library/Application Support/Claude/claude-code/2.1.284/claude.app/Contents/MacOS/claude",
            CLAUDE_NEW,
        );
        assert_eq!(find(&CLAUDE, &mac.lookup()), Some(in_bin));
    }

    #[test]
    fn claude_signed_out_is_reported() {
        let mac = FakeMac::new();
        mac.fake_claude(
            "bin/claude",
            r#"case "$1 $2" in
  "--version ") echo "2.1.286 (Claude Code)" ;;
  "auth --help") printf 'Commands:\n  login [options]  Sign in\n  status [options]  Show authentication status\n' ;;
  "auth status") echo '{"loggedIn": false}'; exit 1 ;;
  *) exit 7 ;;
esac"#,
        );

        let install = detect(&CLAUDE, &mac.lookup()).unwrap();
        assert!(!install.signed_in);
        assert!(!mac.log("bin/claude").contains("-p"));
    }

    #[test]
    fn old_claude_gets_a_no_tools_sample_run() {
        let mac = FakeMac::new();
        mac.fake_claude(
            "bin/claude",
            r#"case "$1" in
  --version) echo "1.0.0 (Claude Code)" ;;
  auth) printf 'Usage: claude [options] [prompt]\n  Check your status with /status\n' ;;
  -p) cat >/dev/null; echo '{"type":"result","subtype":"success","is_error":false,"structured_output":{"ok":true}}' ;;
  *) exit 7 ;;
esac"#,
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
        mac.fake_claude(
            "bin/claude",
            r#"case "$1" in
  --version) echo "1.0.0 (Claude Code)" ;;
  -p) cat >/dev/null; echo '{"type":"result","is_error":true,"result":"Invalid API key"}'; exit 1 ;;
esac"#,
        );

        assert!(!detect(&CLAUDE, &mac.lookup()).unwrap().signed_in);
    }

    #[test]
    fn a_failing_auth_help_is_signed_out_without_a_model_run() {
        let mac = FakeMac::new();
        mac.fake_claude(
            "bin/claude",
            r#"case "$1 $2" in
  "--version ") echo "2.1.286 (Claude Code)" ;;
  "auth --help") exit 1 ;;
  *) cat >/dev/null; echo '{"type":"result","is_error":false,"structured_output":{"ok":true}}' ;;
esac"#,
        );

        assert!(!detect(&CLAUDE, &mac.lookup()).unwrap().signed_in);
        let log = mac.log("bin/claude");
        assert!(!log.contains("-p"), "{log}");
        assert!(!log.contains("auth status"), "{log}");
    }

    #[test]
    fn a_cli_only_folder_is_searched_for_that_cli_only() {
        let mac = FakeMac::new();
        let claude = mac.fake_claude("home/.claude/local/claude", CLAUDE_NEW);
        mac.script("home/.claude/local/codex", "exit 0");

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
        mac.script("bin/codex", "exit 0");
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
