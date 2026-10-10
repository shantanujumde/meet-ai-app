//! Crash files: the panic hook, and the plain writers both crash paths use.
//!
//! A crash file is a few lines of plain text (version, OS, time, what went
//! wrong) named `crash-<UTC yyyymmdd-hhmmss>.log` for a Rust panic and
//! `crash-<ts>-native.log` for a native crash. They sort by name in time
//! order, which is how [`prune`] finds the oldest.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Every crash file's name starts with this.
pub const CRASH_PREFIX: &str = "crash-";

/// The first lines of every crash file: who wrote it and on what.
pub fn header(kind: &str) -> String {
    format!(
        "meet-ai crash report ({kind})\nversion: {}\nos: {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Seconds since the Unix epoch, or 0 if the clock is before 1970.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A UTC date and time, worked out from Unix seconds without allocating, so
/// the native crash path can use it from a crashed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Utc {
    /// Howard Hinnant's `civil_from_days`, public domain.
    pub fn from_unix(secs: u64) -> Self {
        let days = i64::try_from(secs / 86_400).unwrap_or(0);
        let rem = secs % 86_400;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        // Every value below is in range by construction (day 1..=31, month
        // 1..=12, and the rest are remainders of a day), so the casts are exact.
        Self {
            year,
            month: month as u32,
            day: day as u32,
            hour: (rem / 3_600) as u32,
            minute: (rem / 60 % 60) as u32,
            second: (rem % 60) as u32,
        }
    }

    /// `20261003-163804`, the part of a crash file's name that sorts.
    pub fn stamp(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(
            out,
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `2026-10-03T16:38:04Z`, for the `time:` line.
    pub fn rfc3339(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(
            out,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// `crash-<stamp><suffix>.log` inside `dir`.
pub fn crash_path(dir: &Path, at: Utc, suffix: &str) -> PathBuf {
    let mut name = String::from(CRASH_PREFIX);
    let _ = at.stamp(&mut name);
    name.push_str(suffix);
    name.push_str(".log");
    dir.join(name)
}

/// What a panic hook knows about one panic.
pub struct PanicReport<'a> {
    pub thread: &'a str,
    pub message: &'a str,
    pub location: Option<String>,
    pub backtrace: &'a str,
}

/// Write one panic's crash file into `dir` and prune old ones. Returns the
/// file's path. Two panics in the same second share a file, one after the
/// other, rather than overwriting each other.
///
/// Never creates `dir`: it was made at startup, and if it is gone now the
/// meetings folder has moved, and making it again would bring back an empty
/// copy of the old meetings root (TUR-90). [`panic_dir`] picks the folder.
pub fn write_panic_file(dir: &Path, at: Utc, report: &PanicReport<'_>) -> std::io::Result<PathBuf> {
    let path = crash_path(dir, at, "");
    let mut body = header("panic");
    let _ = body.write_str("time: ");
    let _ = at.rfc3339(&mut body);
    let _ = writeln!(body);
    let _ = writeln!(body, "thread: {}", report.thread);
    let _ = writeln!(body, "message: {}", report.message);
    let _ = writeln!(
        body,
        "location: {}",
        report.location.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(body, "backtrace:\n{}", report.backtrace);
    append(&path, body.as_bytes())?;
    prune(dir, super::MAX_CRASH_FILES);
    Ok(path)
}

/// Write one native crash's file into `dir`. No pruning and no heap
/// allocation on Unix: this runs inside a crashed process (a signal handler on
/// Linux, with every other thread suspended on macOS), where `malloc` may be
/// holding a lock it will never release. Pruning happens at the next launch.
///
/// `dir` is a `str` so the path can be built in a stack buffer; the caller
/// checks it is UTF-8 once, before the crash.
pub fn write_native_file(
    dir: &str,
    header: &str,
    at: Utc,
    exception: &dyn std::fmt::Display,
) -> std::io::Result<()> {
    let mut path = StackStr::<1024>::new();
    let built = (|| {
        path.write_str(dir)?;
        path.write_char(std::path::MAIN_SEPARATOR)?;
        path.write_str(CRASH_PREFIX)?;
        at.stamp(&mut path)?;
        path.write_str("-native.log")
    })();
    if built.is_err() {
        return Err(std::io::Error::other("crash file path too long"));
    }
    let mut file = open_append(Path::new(path.as_str()))?;
    file.write_all(header.as_bytes())?;
    let mut time = StackStr::<32>::new();
    let _ = at.rfc3339(&mut time);
    writeln!(file, "time: {}", time.as_str())?;
    writeln!(file, "exception: {exception}")?;
    file.flush()
}

/// Open `path` for appending, creating it if needed. Does not allocate, so
/// the native crash path can use it too.
fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

fn append(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    open_append(path)?.write_all(bytes)
}

/// Keep the newest `keep` crash files in `dir`, deleting the oldest first.
/// Best effort: a file that cannot be deleted is left alone.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with(CRASH_PREFIX) && name.ends_with(".log"))
        .collect();
    if names.len() <= keep {
        return;
    }
    // The stamp right after the prefix sorts in time order.
    names.sort();
    let excess = names.len() - keep;
    for name in names.into_iter().take(excess) {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// The folder a panic's file goes in: `dir` while it is still there, else
/// `fallback` (the OS log folder) if that is. `None` when neither is: the
/// panic then goes to stderr only.
pub fn panic_dir<'a>(dir: &'a Path, fallback: Option<&'a Path>) -> Option<&'a Path> {
    if dir.is_dir() {
        Some(dir)
    } else {
        fallback.filter(|fallback| fallback.is_dir())
    }
}

/// Write a crash file into the folder `dir` names for every panic, then run
/// the hook that was there before (the default one prints to stderr), so
/// nothing else changes. `dir` is asked at each panic, so a folder move this
/// launch is followed (TUR-149). When that folder is not there, the file goes
/// to the folder `fallback` names instead; see [`panic_dir`].
///
/// A panic that the app catches and recovers from is still written: it is a
/// bug either way, and [`MAX_CRASH_FILES`](super::MAX_CRASH_FILES) caps how
/// many pile up.
pub fn install_panic_hook(
    dir: impl Fn() -> PathBuf + Send + Sync + 'static,
    fallback: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let report = PanicReport {
            thread: thread.name().unwrap_or("unnamed"),
            message: info.payload_as_str().unwrap_or("(no message)"),
            location: info
                .location()
                .map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column())),
            backtrace: &backtrace,
        };
        // No tracing call here: a panic inside the logger would recurse.
        let dir = dir();
        let fallback = fallback();
        let written = match panic_dir(&dir, fallback.as_deref()) {
            Some(dir) => write_panic_file(dir, Utc::from_unix(unix_now()), &report).map(drop),
            None => Err(std::io::Error::other(
                "neither the logs folder nor the OS log folder is there",
            )),
        };
        if let Err(error) = written {
            eprintln!("meet-ai: could not write the crash file: {error}");
        }
        previous(info);
    }));
}

/// A fixed-size string on the stack, for building text without allocating.
pub struct StackStr<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackStr<N> {
    pub fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // Only whole `&str`s are ever copied in, so this is always UTF-8.
        std::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Default for StackStr<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> std::fmt::Write for StackStr<N> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(std::fmt::Error)?;
        let slot = self.buf.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        slot.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
