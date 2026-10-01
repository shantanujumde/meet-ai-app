//! Resource limits.

use super::*;

/// Env var that makes a test run its dangerous half in a child process.
const CHILD: &str = "MEET_AI_STORE_ADVERSARIAL_CHILD";

/// Run the named test again in a child copy of this test binary, with
/// [`CHILD`] set. A stack overflow aborts the whole process and cannot be
/// caught, so this is the only way to report one as a plain failure instead
/// of taking every other test down with it.
fn run_in_a_child(test: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--ignored", "--test-threads=1"])
        .env(CHILD, "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
}

#[test]
fn deeply_nested_frontmatter_loads_flagged_instead_of_aborting_the_process() {
    // ~40 KB: `k:` followed by 20 000 levels of nesting. yaml-rust2's
    // loader recurses once per level; about 1 000 levels (2 KB) is already
    // enough on a 2 MB test thread in a debug build.
    if std::env::var_os(CHILD).is_some() {
        // Two spellings of the same nesting: compact block lists, and
        // compact mappings with empty keys (`: : : x`).
        for unit in ["- ", ": "] {
            let raw = format!("---\nk:\n  {}x\n---\n## Summary\n", unit.repeat(20_000));
            let meeting = Meeting::parse(&raw);
            assert!(meeting.section("Summary").is_some());
            let _ = Ticket::parse(&raw);
        }
        return;
    }
    let status = run_in_a_child(
        "resource_limits::deeply_nested_frontmatter_loads_flagged_instead_of_aborting_the_process",
    );
    assert!(
        status.success(),
        "parsing the file killed the process: {status}"
    );
}

#[test]
fn a_small_file_of_nested_aliases_is_not_expanded_into_megabytes() {
    // Five levels of ten aliases each: about 300 bytes of frontmatter, 10^5
    // nodes once expanded. Seven levels would be 10^7 and exhaust memory.
    let mut raw = String::from("---\nid: m1\ntitle: T\na0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..5 {
        let refs = vec![format!("*a{}", i - 1); 10].join(", ");
        raw.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
    }
    raw.push_str("---\n");
    let meeting = Meeting::parse(&raw);
    if is_refused(&meeting.problems) {
        return; // Refusing the file is a fine answer.
    }
    let rendered = meeting.render().unwrap();
    assert!(
        rendered.len() < 64 * 1024,
        "{} bytes in, {} bytes out",
        raw.len(),
        rendered.len()
    );
}

#[test]
fn flow_nesting_up_to_the_loaders_own_limit_does_not_crash() {
    // yaml-rust2 caps flow nesting (`[[[...`) at 255 levels itself; make
    // sure everything store does with such a value fits a test thread's
    // stack.
    for depth in [100, 254, 255, 256, 1000] {
        let raw = format!("---\nk: {}x{}\n---\n", "[".repeat(depth), "]".repeat(depth));
        let meeting = Meeting::parse(&raw);
        if let Ok(text) = meeting.render() {
            assert_eq!(Meeting::parse(&text).frontmatter, meeting.frontmatter);
        }
        let _ = Ticket::parse(&raw).render();
    }
}

/// Parse, render and re-parse one very long line of `repeats` units. Returns
/// how long the whole round trip took.
fn long_line_round_trip(repeats: usize) -> std::time::Duration {
    let start = std::time::Instant::now();
    let long = "é🎉 [x]: ".repeat(repeats);
    let raw = format!("---\nid: m1\ntitle: \"{long}\"\n---\n## Summary\n{long}\n");
    let meeting = Meeting::parse(&raw);
    assert_eq!(meeting.title().as_deref(), Some(long.as_str()));
    let text = meeting.render().unwrap();
    assert_eq!(Meeting::parse(&text).sections, meeting.sections);
    let line = transcript::format_line(1, Speaker::You, &long).unwrap();
    assert!(transcript::parse_line(&line).is_some());
    start.elapsed()
}

/// A wall-clock limit is flaky on a shared debug-build runner, so this checks
/// the shape instead: 4x the input must not cost much more than 4x the time.
/// Linear parsing gives a ratio near 4; quadratic parsing gives about 16.
#[test]
fn a_very_long_line_parses_in_reasonable_time() {
    const SMALL: usize = 25_000;
    let small = (0..3).map(|_| long_line_round_trip(SMALL)).min().unwrap();
    let big = long_line_round_trip(SMALL * 4);
    // Allow 10x (linear is 4x, quadratic 16x) plus a flat 1 s for timer noise.
    let limit = small * 10 + std::time::Duration::from_secs(1);
    assert!(
        big < limit,
        "4x input took {big:?}, 1x took {small:?}: parsing is no longer linear"
    );
}
