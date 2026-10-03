use std::path::Path;

use super::crash::{
    CRASH_PREFIX, PanicReport, StackStr, Utc, crash_path, install_panic_hook, prune,
    write_native_file, write_panic_file,
};
use super::*;

fn crash_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with(CRASH_PREFIX))
        .collect();
    names.sort();
    names
}

fn report<'a>(message: &'a str) -> PanicReport<'a> {
    PanicReport {
        thread: "main",
        message,
        location: Some("src/lib.rs:1:2".into()),
        backtrace: "   0: meet_ai_lib::run",
    }
}

#[test]
fn unix_seconds_become_the_right_utc_date() {
    assert_eq!(
        Utc::from_unix(0),
        Utc {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0
        }
    );
    // 2026-10-03T16:38:04Z, checked against `date -u -r 1791045484`.
    let at = Utc::from_unix(1_791_045_484);
    let mut stamp = String::new();
    at.stamp(&mut stamp).unwrap();
    assert_eq!(stamp, "20261003-163804");
    let mut time = String::new();
    at.rfc3339(&mut time).unwrap();
    assert_eq!(time, "2026-10-03T16:38:04Z");
    // A leap day, and the last second of a year.
    let mut leap = String::new();
    Utc::from_unix(951_825_600).rfc3339(&mut leap).unwrap();
    assert_eq!(leap, "2000-02-29T12:00:00Z");
    let mut eoy = String::new();
    Utc::from_unix(1_798_761_599).rfc3339(&mut eoy).unwrap();
    assert_eq!(eoy, "2026-12-31T23:59:59Z");
}

#[test]
fn a_panic_file_says_what_where_when_and_which_version() {
    let dir = tempfile::tempdir().unwrap();
    let at = Utc::from_unix(1_791_045_484);
    let path = write_panic_file(dir.path(), at, &report("index out of bounds")).unwrap();

    assert_eq!(path, dir.path().join("crash-20261003-163804.log"));
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.starts_with("meet-ai crash report (panic)\n"), "{body}");
    assert!(body.contains(&format!("version: {}\n", env!("CARGO_PKG_VERSION"))));
    assert!(body.contains(&format!("os: {} ", std::env::consts::OS)));
    assert!(body.contains("time: 2026-10-03T16:38:04Z\n"));
    assert!(body.contains("thread: main\n"));
    assert!(body.contains("message: index out of bounds\n"));
    assert!(body.contains("location: src/lib.rs:1:2\n"));
    assert!(body.contains("backtrace:\n   0: meet_ai_lib::run"));
}

#[test]
fn two_panics_in_one_second_share_a_file_instead_of_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let at = Utc::from_unix(1_791_045_484);
    write_panic_file(dir.path(), at, &report("first")).unwrap();
    let path = write_panic_file(dir.path(), at, &report("second")).unwrap();

    let body = std::fs::read_to_string(path).unwrap();
    assert!(body.contains("message: first\n") && body.contains("message: second\n"));
}

#[test]
fn a_native_crash_file_names_the_exception() {
    let dir = tempfile::tempdir().unwrap();
    let at = Utc::from_unix(1_791_045_484);
    write_native_file(
        dir.path().to_str().unwrap(),
        &crash::header("native"),
        at,
        &"EXC_BAD_ACCESS (kind 1), code 0x1",
    )
    .unwrap();

    let path = dir.path().join("crash-20261003-163804-native.log");
    assert_eq!(path, crash_path(dir.path(), at, "-native"));
    let body = std::fs::read_to_string(path).unwrap();
    assert!(
        body.starts_with("meet-ai crash report (native)\n"),
        "{body}"
    );
    assert!(body.contains(&format!("version: {}\n", env!("CARGO_PKG_VERSION"))));
    assert!(body.contains("time: 2026-10-03T16:38:04Z\n"));
    assert!(body.contains("exception: EXC_BAD_ACCESS (kind 1), code 0x1\n"));
}

#[test]
fn a_native_path_too_long_for_the_stack_buffer_is_an_error_not_a_crash() {
    let long = "x".repeat(2_000);
    let result = write_native_file(&long, "", Utc::from_unix(0), &"signal 11");
    assert!(result.is_err());
}

#[test]
fn only_the_newest_crash_files_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    for minute in 0..7_u64 {
        let at = Utc::from_unix(1_791_045_484 + minute * 60);
        let suffix = if minute % 2 == 0 { "" } else { "-native" };
        std::fs::write(crash_path(dir.path(), at, suffix), "x").unwrap();
    }
    // Not crash files: the log and its rotated copy are never touched.
    std::fs::write(dir.path().join("meet-ai.log"), "log").unwrap();
    std::fs::write(dir.path().join("meet-ai_2026-10-03.log"), "old").unwrap();

    prune(dir.path(), MAX_CRASH_FILES);

    let kept = crash_files(dir.path());
    assert_eq!(kept.len(), MAX_CRASH_FILES);
    // The two oldest (16:38 and 16:39) went first.
    assert_eq!(kept[0], "crash-20261003-164004.log");
    assert!(dir.path().join("meet-ai.log").exists());
    assert!(dir.path().join("meet-ai_2026-10-03.log").exists());
}

#[test]
fn writing_a_sixth_panic_file_deletes_the_oldest() {
    let dir = tempfile::tempdir().unwrap();
    for minute in 0..MAX_CRASH_FILES as u64 {
        let at = Utc::from_unix(1_791_045_484 + minute * 60);
        std::fs::write(crash_path(dir.path(), at, ""), "x").unwrap();
    }
    let newest = Utc::from_unix(1_791_045_484 + 3_600);
    write_panic_file(dir.path(), newest, &report("boom")).unwrap();

    let kept = crash_files(dir.path());
    assert_eq!(kept.len(), MAX_CRASH_FILES);
    assert!(!kept.contains(&"crash-20261003-163804.log".to_string()));
    assert!(kept.contains(&"crash-20261003-173804.log".to_string()));
}

/// The real hook, on a real panic: the CI check that a forced panic leaves a
/// readable file on every OS. The hook is process-wide, so the assertion
/// looks for this test's own message rather than counting files.
#[test]
fn a_real_panic_leaves_a_crash_file() {
    let dir = tempfile::tempdir().unwrap();
    install_panic_hook(dir.path().to_path_buf());

    let joined = std::thread::Builder::new()
        .name("tur46-forced-panic".into())
        .spawn(|| panic!("TUR-46 forced panic"))
        .unwrap()
        .join();
    // Back to the default hook, so a later test's panic does not write into
    // (and recreate) this temp folder after it is gone.
    drop(std::panic::take_hook());
    assert!(joined.is_err());

    let found = crash_files(dir.path()).into_iter().any(|name| {
        let body = std::fs::read_to_string(dir.path().join(name)).unwrap();
        body.contains("message: TUR-46 forced panic\n")
            && body.contains("thread: tur46-forced-panic\n")
            && body.contains("location: ")
            && body.contains("tests.rs:")
            && body.contains("backtrace:\n")
    });
    assert!(found, "no crash file for the forced panic");
}

#[test]
fn the_stack_string_refuses_to_overflow() {
    use std::fmt::Write as _;
    let mut text = StackStr::<4>::new();
    assert!(text.write_str("abcd").is_ok());
    assert!(text.write_str("e").is_err());
    assert_eq!(text.as_str(), "abcd");
}

#[test]
fn the_log_is_capped_at_a_megabyte() {
    assert_eq!(LOG_MAX_BYTES, 1_000_000);
    assert_eq!(MAX_CRASH_FILES, 5);
    assert_eq!(LOG_FILE_STEM, "meet-ai");
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[test]
fn mach_exception_kinds_have_their_header_names() {
    assert_eq!(native::mach_exception_name(1), "EXC_BAD_ACCESS");
    assert_eq!(native::mach_exception_name(10), "EXC_CRASH");
    assert_eq!(native::mach_exception_name(99), "unknown exception");
}
