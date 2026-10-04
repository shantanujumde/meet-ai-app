//! `fake-cli`: a pretend agent CLI for tests, on every OS (TUR-54).
//!
//! It replaces the `/bin/sh` scripts the agent tests used to run. No
//! dependencies, so `build.rs` can compile it with a bare `rustc` and every
//! test in the workspace can find it through `test_support::fake_cli_path`.
//!
//! What it does is read from settings. Setting `name` comes from the
//! environment variable `FAKE_<NAME>` if that is set, or else from the file
//! `<exe folder>/<exe stem>.fake/<name>` (see `test_support::FakeCli`), so a
//! copy of this program at a path a test hands to the code under test can be
//! configured without touching the test process's environment.
//!
//! In order, each step only when its setting is there:
//!
//! - `touch`: creates this file.
//! - `log_dir`: writes `args.log` (the arguments joined by spaces, then a
//!   newline), `cwd.log` (the working folder, then how many entries it holds)
//!   and `stdin.log` (all of stdin) into this folder. Without it, stdin is
//!   read to its end and dropped, or echoed to stdout when `echo_stdin` is
//!   `1`.
//! - `log_append`: appends the arguments joined by spaces, then a newline, to
//!   this file (one line per call).
//! - `expect_args`: exits 7 unless the arguments joined by spaces are this.
//! - `cases`: replies picked by the arguments, one per line:
//!   `<prefix>\t<code>\t<stdout>\t<stderr>`, where `\n` in the two texts is
//!   a newline. The first line whose prefix starts the arguments joined by
//!   spaces wins (`*` matches anything) and replaces `stdout`, `stderr` and
//!   `code`; no match exits 7.
//! - `print_cwd`: `1` prints the working folder and its entry count.
//! - `grandchild_pid_file`: starts a copy of itself that sleeps 30 s and
//!   writes that copy's pid to this file. With `grandchild_keeps_stdout` set
//!   to `1` the copy shares our stdout, so it holds the pipe open; otherwise
//!   its stdio is null.
//! - `sleep`: sleeps this many seconds (fractions allowed).
//! - `stdout_pad`: prints this many `0` bytes before `stdout`.
//! - `stdout`: printed as-is.
//! - `stderr_pad`: prints this many `a` before `stderr`.
//! - `stderr`: printed as-is on stderr.
//! - `code`: the exit code (default 0).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

/// Set on the sleeping copy that `grandchild_pid_file` starts.
const SLEEPER: &str = "FAKE_CLI_SLEEPER";

fn main() -> ExitCode {
    if std::env::var_os(SLEEPER).is_some() {
        std::thread::sleep(Duration::from_secs(30));
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("fake-cli: {e}");
            ExitCode::from(101)
        }
    }
}

fn run() -> io::Result<u8> {
    let settings = Settings::new()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = args.join(" ");

    if let Some(path) = settings.get("touch") {
        std::fs::write(path, "")?;
    }

    let mut stdin = Vec::new();
    io::stdin().read_to_end(&mut stdin)?;
    if let Some(dir) = settings.get("log_dir") {
        let dir = PathBuf::from(dir);
        std::fs::write(dir.join("args.log"), format!("{args}\n"))?;
        std::fs::write(dir.join("cwd.log"), cwd_report()?)?;
        std::fs::write(dir.join("stdin.log"), &stdin)?;
    }

    if let Some(path) = settings.get("log_append") {
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(log, "{args}")?;
    }

    let case = match settings.get("cases") {
        Some(cases) => match pick_case(&cases, &args) {
            Some(case) => Some(case),
            None => return Ok(7),
        },
        None => None,
    };

    if let Some(expected) = settings.get("expect_args")
        && expected != args
    {
        return Ok(7);
    }

    let mut out = io::stdout().lock();
    if settings.get("echo_stdin").as_deref() == Some("1") {
        out.write_all(&stdin)?;
    }
    if settings.get("print_cwd").as_deref() == Some("1") {
        out.write_all(cwd_report()?.as_bytes())?;
    }
    out.flush()?;

    if let Some(pid_file) = settings.get("grandchild_pid_file") {
        let keeps_stdout = settings.get("grandchild_keeps_stdout").as_deref() == Some("1");
        let mut sleeper = Command::new(std::env::current_exe()?);
        sleeper
            .env(SLEEPER, "1")
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        if !keeps_stdout {
            sleeper.stdout(Stdio::null());
        }
        let child = sleeper.spawn()?;
        std::fs::write(pid_file, child.id().to_string())?;
        // Not waited for: the test checks it is killed with us.
        std::mem::forget(child);
    }

    if let Some(secs) = settings.get("sleep") {
        let secs: f64 = secs.trim().parse().map_err(io::Error::other)?;
        std::thread::sleep(Duration::from_secs_f64(secs));
    }

    if let Some(n) = settings.number("stdout_pad")? {
        write_repeated(&mut out, b'0', n)?;
    }
    let stdout = case
        .as_ref()
        .map(|c| c.stdout.clone())
        .or_else(|| settings.get("stdout"));
    if let Some(text) = stdout {
        out.write_all(text.as_bytes())?;
    }
    out.flush()?;

    let mut err = io::stderr().lock();
    if let Some(n) = settings.number("stderr_pad")? {
        write_repeated(&mut err, b'a', n)?;
    }
    let stderr = case
        .as_ref()
        .map(|c| c.stderr.clone())
        .or_else(|| settings.get("stderr"));
    if let Some(text) = stderr {
        err.write_all(text.as_bytes())?;
    }
    err.flush()?;

    if let Some(case) = case {
        return Ok(case.code);
    }

    match settings.number("code")? {
        Some(code) => u8::try_from(code).map_err(io::Error::other),
        None => Ok(0),
    }
}

/// One line of the `cases` setting.
struct Case {
    code: u8,
    stdout: String,
    stderr: String,
}

/// The first case whose prefix starts `args`.
fn pick_case(cases: &str, args: &str) -> Option<Case> {
    cases.lines().find_map(|line| {
        let mut parts = line.split('\t');
        let prefix = parts.next()?;
        if prefix != "*" && !args.starts_with(prefix) {
            return None;
        }
        let code = parts.next().unwrap_or("0").trim().parse().unwrap_or(101);
        let unescape = |s: Option<&str>| s.unwrap_or("").replace("\\n", "\n");
        Some(Case {
            code,
            stdout: unescape(parts.next()),
            stderr: unescape(parts.next()),
        })
    })
}

/// The working folder, canonical, then how many entries it holds.
fn cwd_report() -> io::Result<String> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let entries = std::fs::read_dir(&cwd)?.count();
    Ok(format!("{}\n{entries}\n", cwd.display()))
}

fn write_repeated(to: &mut impl Write, byte: u8, n: usize) -> io::Result<()> {
    let chunk = [byte; 64 * 1024];
    let mut left = n;
    while left > 0 {
        let take = left.min(chunk.len());
        to.write_all(&chunk[..take])?;
        left -= take;
    }
    Ok(())
}

/// Where the settings come from: the environment first, then the config
/// folder next to this program.
struct Settings {
    dir: Option<PathBuf>,
}

impl Settings {
    fn new() -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        let dir = config_dir(&exe);
        Ok(Self {
            dir: dir.filter(|d| d.is_dir()),
        })
    }

    fn get(&self, name: &str) -> Option<String> {
        if let Ok(value) = std::env::var(format!("FAKE_{}", name.to_uppercase())) {
            return Some(value);
        }
        let dir = self.dir.as_ref()?;
        std::fs::read_to_string(dir.join(name)).ok()
    }

    fn number(&self, name: &str) -> io::Result<Option<usize>> {
        self.get(name)
            .map(|v| v.trim().parse().map_err(io::Error::other))
            .transpose()
    }
}

/// `<folder>/<stem>.fake` for the program at `<folder>/<stem>[.exe]`.
fn config_dir(exe: &Path) -> Option<PathBuf> {
    let stem = exe.file_stem()?.to_string_lossy().into_owned();
    Some(exe.with_file_name(format!("{stem}.fake")))
}
