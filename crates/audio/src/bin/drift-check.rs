//! `drift-check` — reads a finished recording and reports the SPEC §5 drift
//! gate, or refuses to certify one that cannot be measured (contract §12).
//!
//! `cargo run -p audio --bin drift-check -- <DIR>` reads `<DIR>/segments.json`
//! plus `<DIR>/mic.wav` and `<DIR>/system.wav` (when present), and never the
//! other way around: the gate number comes from [`audio::segments::Segments::drift`],
//! which is built entirely from `segments.json`'s checkpoint anchors against
//! the host clock — never from `(mic_frames - sys_frames)`, the subtraction
//! contract §11 replaced because it can cancel a common-mode drift to ~0.
//!
//! Exit codes:
//! - `0` — measured, both channels under [`segments::DRIFT_GATE_MS`].
//! - `1` — measured, at least one channel over the gate.
//! - `2` — **not measurable**: `segments.json` doesn't carry enough to
//!   compute a number at all (see [`segments::DriftError`] for the six
//!   reasons), printed on stderr as `not measurable: <reason>` rather than
//!   guessing a number that would flatter a broken recording.

use std::path::Path;
use std::process::ExitCode;

use audio::Channel;
use audio::segments::{DRIFT_GATE_MS, Segments, SegmentsExt as _};
use audio::wav_writer::read_header_frames;

const USAGE: &str = "\
drift-check — measure drift between mic.wav and system.wav (SPEC §5 gate).

USAGE:
    drift-check <DIR>

Reads <DIR>/segments.json (and <DIR>/mic.wav, <DIR>/system.wav, if present).

EXIT CODES:
    0   measured, under the 200ms gate
    1   measured, over the 200ms gate
    2   not measurable — see stderr for why
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(dir) => run(Path::new(dir)),
        None => {
            eprintln!("drift-check: a directory argument is required");
            eprintln!();
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(dir: &Path) -> ExitCode {
    let segments_path = dir.join(meeting_format::layout::SEGMENTS_FILE);
    let json = match std::fs::read_to_string(&segments_path) {
        Ok(json) => json,
        Err(e) => {
            eprintln!(
                "not measurable: could not read {}: {e}",
                segments_path.display()
            );
            return ExitCode::from(2);
        }
    };
    let segments = match Segments::from_json(&json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "not measurable: {} does not parse: {e}",
                segments_path.display()
            );
            return ExitCode::from(2);
        }
    };

    // The invariant this exact bug (TUR-54) broke: sum(*_frames) >= header
    // frames, always. Checked and reported for both channels, unconditionally
    // — informational rather than gating, since contract §12's refusal list
    // is about anchor coverage, not this — but surfacing a violation here is
    // exactly what would have caught it at record time instead of by manual
    // byte-comparison after the fact.
    for channel in [Channel::Mic, Channel::System] {
        if segments.channel_absent(channel) {
            continue;
        }
        let wav_path = dir.join(channel.wav_filename());
        match read_header_frames(&wav_path) {
            Ok(header_frames) => {
                if let Err(violation) = segments.check_wav_header(channel, header_frames) {
                    eprintln!("drift-check: invariant violation: {violation}");
                }
            }
            Err(e) => {
                eprintln!("drift-check: could not read {}: {e}", wav_path.display());
            }
        }
    }

    let report = match segments.drift() {
        Ok(report) => report,
        Err(e) => {
            eprintln!("not measurable: {e}");
            return ExitCode::from(2);
        }
    };

    println!(
        "drift-check: mic — max {:.1}ms, final {:.1}ms, {} anchors, tail unanchored {:.1}ms",
        report.mic.max_abs_ms,
        report.mic.final_ms,
        report.mic.anchors,
        report.mic.tail_unanchored_ms
    );
    println!(
        "drift-check: system — max {:.1}ms, final {:.1}ms, {} anchors, tail unanchored {:.1}ms",
        report.system.max_abs_ms,
        report.system.final_ms,
        report.system.anchors,
        report.system.tail_unanchored_ms
    );
    // Printed unconditionally, including on a failing exit — contract §12/F4:
    // this is what separates a genuine two-track divergence from a common-mode
    // failure (both channels sliding together), which the gate above catches
    // but this number alone would have missed.
    println!(
        "drift-check: cross-track skew (informational, not the gate) — {:.1}ms",
        report.max_track_skew_ms
    );
    for gap in &report.boundary_gaps {
        let asleep = gap
            .asleep_ms
            .map(|ms| format!(", {ms:.0}ms asleep"))
            .unwrap_or_default();
        println!(
            "drift-check: boundary ({}) — mic gap {:.0}ms, system gap {:.0}ms{asleep}",
            gap.reason, gap.mic_ms, gap.sys_ms
        );
    }
    for breach in [report.mic.first_breach, report.system.first_breach]
        .into_iter()
        .flatten()
    {
        println!(
            "drift-check: crossed the {DRIFT_GATE_MS:.0}ms gate at {:.1}s ({:.1}ms)",
            breach.elapsed_s, breach.drift_ms
        );
    }

    // §12/F4: drift re-anchors at every segment boundary, so this is the max
    // over segments, never the sum. Do not "fix" this into an accumulation —
    // that would fail clean recordings that pass today.
    let worst = report.worst_ms();
    if report.passes(DRIFT_GATE_MS) {
        println!("drift-check: PASS — worst {worst:.1}ms, under the {DRIFT_GATE_MS:.0}ms gate");
        ExitCode::SUCCESS
    } else {
        println!("drift-check: FAIL — worst {worst:.1}ms, over the {DRIFT_GATE_MS:.0}ms gate");
        ExitCode::FAILURE
    }
}
