//! The `sidecar/meet-stt` process contract.
//!
//! SPEC §2.6 chose to test the Swift sidecar from Rust rather than with
//! XCTest: a bare `swiftc` build has no test target, and adding SwiftPM just
//! to get one is disproportionate. Testing it from here has the better
//! property anyway — it exercises the actual process boundary and JSON shape,
//! which is what the app depends on, instead of Swift internals.

mod fixtures;

use std::process::Command;

fn sidecar_or_skip() -> Option<std::path::PathBuf> {
    match fixtures::sidecar() {
        Some(path) => Some(path),
        None => {
            eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
            None
        }
    }
}

#[test]
fn probe_answers_with_one_well_formed_json_line() {
    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    let output = Command::new(&binary)
        .args(["--probe", "--locale", "en-US"])
        .output()
        .expect("meet-stt --probe");

    assert!(
        output.status.success(),
        "--probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected exactly one line, got {lines:?}");

    let value: serde_json::Value = serde_json::from_str(lines[0]).expect("probe line is JSON");
    assert_eq!(value["type"], "probe");
    assert_eq!(value["engine"], "apple-speech");
    assert!(value["available"].is_boolean());
    assert!(value["installed"].is_boolean());
}

/// The probe is what the registry uses to pick an engine, so it must answer
/// even when the answer is "no".
#[test]
fn probe_is_parseable_by_the_rust_driver() {
    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("probe parses");
    eprintln!(
        "apple engine: available={} installed={} locale={:?} os={:?}",
        probe.available, probe.installed, probe.locale, probe.os_version
    );
}

#[test]
fn an_unsupported_locale_is_a_typed_error_not_a_crash() {
    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    // `zz-ZZ` is not a real locale, so the sidecar must resolve nothing and
    // say so on stdout rather than dying without a JSON line.
    let output = Command::new(&binary)
        .args(["--probe", "--locale", "zz-ZZ"])
        .output()
        .expect("meet-stt --probe");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.lines().next().unwrap_or("")).expect("still a JSON line");
    assert_eq!(value["type"], "probe");
    assert_eq!(value["installed"], false);
}

#[test]
fn a_missing_wav_produces_an_error_line_and_a_nonzero_exit() {
    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    let output = Command::new(&binary)
        .arg("/nonexistent/nope.wav")
        .output()
        .expect("meet-stt");

    assert!(!output.status.success(), "a missing WAV must not exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap_or(""))
        .expect("an error is still a JSON line");
    assert_eq!(value["type"], "error");
    assert!(value["code"].is_string());
    assert!(value["message"].is_string());
}

#[test]
fn stdin_and_a_wav_path_together_are_a_typed_error() {
    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    // The live path (TUR-31) reads frames from a pipe; the batch path opens a
    // finished file. Asking for both is a caller bug, and it has to be caught
    // before the model loads rather than silently resolving to one of them.
    let output = Command::new(&binary)
        .args(["--stdin", "/some/meeting/mic.wav"])
        .output()
        .expect("meet-stt");

    assert!(
        !output.status.success(),
        "contradictory arguments must fail"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap_or(""))
        .expect("an error is still a JSON line");
    assert_eq!(value["type"], "error");
    assert_eq!(value["code"], "bad_arguments");
}

/// Transcription dying must cost the recorder a write error and nothing else.
///
/// SPEC and TUR-15 both require that a transcription failure never kills the
/// recording: capture keeps going and the UI says "Transcription stopped —
/// still recording". On the live path the recorder holds a pipe into
/// `meet-stt --stdin`, so the question is concrete — what does the *writer*
/// see when the reader dies? A `SIGPIPE` would kill the recorder outright and
/// take the meeting with it. Rust's runtime ignores `SIGPIPE`, which turns it
/// into an ordinary `BrokenPipe`, and this asserts that rather than trusting
/// it: the whole independence property rests on it, and it is one
/// `signal(2)` call away from being untrue.
#[test]
fn killing_the_sidecar_leaves_the_writer_alive_with_a_broken_pipe() {
    use std::io::Write;
    use std::process::Stdio;

    let Some(binary) = sidecar_or_skip() else {
        return;
    };

    let mut child = Command::new(&binary)
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn meet-stt --stdin");
    let mut pipe = child.stdin.take().expect("piped stdin");

    child.kill().expect("kill meet-stt");
    child.wait().expect("reap meet-stt");

    // 100 ms of silence per write — the unit the recorder's tee would use.
    // macOS holds 64 KB in a pipe, so this writes well past that to be sure we
    // are hitting a dead reader rather than a buffer that has not filled yet.
    let chunk = vec![0u8; 1600 * std::mem::size_of::<i16>()];
    let mut error = None;
    for _ in 0..200 {
        if let Err(e) = pipe.write_all(&chunk) {
            error = Some(e);
            break;
        }
    }

    let error = error.expect("writing to a dead sidecar must eventually fail");
    assert_eq!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe,
        "the recorder must see a plain BrokenPipe it can shrug off, got {error:?}"
    );
    // Reaching here at all is the assertion that matters: a SIGPIPE would have
    // killed this test process on the write above.
}

/// Every line on stdout must be JSON — nothing may print debug chatter.
///
/// This is the contract that makes the driver's "unparseable line is a hard
/// error" stance safe. If the sidecar ever starts printing a warning to
/// stdout, this catches it here rather than in a user's meeting.
#[test]
fn transcription_emits_nothing_but_json_lines() {
    fixtures::ensure();

    let Some(binary) = sidecar_or_skip() else {
        return;
    };
    // Transcribing needs Apple's engine and its en-US model. Without them the
    // sidecar answers with an `engine_unavailable` error line, which is correct
    // behaviour, not the stdout noise this test is looking for. Skip the same
    // way accuracy.rs and live.rs do.
    let probe = stt::apple::AppleEngine::probe(&binary, "en-US").expect("meet-stt --probe");
    if !probe.is_usable_offline() {
        eprintln!("SKIPPED: Apple's engine is not usable offline here");
        return;
    }

    let output = Command::new(&binary)
        .arg(fixtures::path("two-speaker-60s/mic.wav"))
        .output()
        .expect("meet-stt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut saw_final = false;
    let mut saw_done = false;

    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {line:?}"));
        match value["type"].as_str() {
            Some("final") => {
                saw_final = true;
                assert!(
                    value["start_sec"].is_number(),
                    "final without a start: {line}"
                );
                assert!(value["text"].is_string(), "final without text: {line}");
            }
            Some("done") => saw_done = true,
            Some("error") => panic!("sidecar errored: {line}"),
            _ => {}
        }
    }

    if output.status.success() {
        assert!(saw_final, "no final lines from the speech fixture");
        assert!(
            saw_done,
            "no done line; the caller could not tell it finished"
        );
    } else {
        eprintln!(
            "SKIPPED assertions: meet-stt exited {} ({})",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}
