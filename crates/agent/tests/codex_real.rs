//! One notes run against the real, signed-in `codex` CLI. It makes a model
//! call on the user's own account, so it never runs on its own:
//!
//! ```sh
//! MEET_AI_CODEX=/path/to/codex cargo test -p agent --test codex_real -- --ignored --nocapture
//! ```
//!
//! Without `MEET_AI_CODEX` it runs `codex` from `PATH`.

use std::time::{Duration, Instant};

use agent::{CodexHarness, Harness, Job};
use serde_json::{Value, json};

const SAMPLE: &str = "Summarize this meeting as JSON matching the schema.\n\
[00:00:05] Priya: I'll draft the release notes by Friday.\n\
[00:00:12] Sam: We decided to ship on the 14th.\n\
[00:00:20] Priya: Who writes the blog post?";

fn notes_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "decisions", "open_questions", "tasks"],
        "properties": {
            "summary": { "type": "string" },
            "decisions": { "type": "array", "items": { "type": "string" } },
            "open_questions": { "type": "array", "items": { "type": "string" } },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "details", "owner", "due", "transcript_ref"],
                    "properties": {
                        "title": { "type": "string" },
                        "details": { "type": "string" },
                        "owner": { "type": ["string", "null"] },
                        "due": { "type": ["string", "null"] },
                        "transcript_ref": { "type": "string", "pattern": "^\\d{2}:\\d{2}:\\d{2}$" }
                    }
                }
            }
        }
    })
}

/// `HH:MM:SS`, two digits each.
fn is_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 8
        && bytes.iter().enumerate().all(|(i, b)| match i {
            2 | 5 => *b == b':',
            _ => b.is_ascii_digit(),
        })
}

#[test]
#[ignore = "needs a signed-in codex CLI; run by hand"]
fn a_real_codex_turns_a_sample_transcript_into_notes() {
    let binary = std::env::var_os("MEET_AI_CODEX").unwrap_or_else(|| "codex".into());
    let codex = CodexHarness::with_binary(binary);
    let mut job = Job::notes(SAMPLE, notes_schema());
    job.timeout = Duration::from_secs(120);
    job.model = None;

    let start = Instant::now();
    let result = codex.run(&job);
    println!("codex notes run took {:?}", start.elapsed());

    let notes = result.unwrap();
    println!("{notes:#}");
    let tasks = notes["tasks"].as_array().unwrap();
    assert!(!tasks.is_empty(), "{notes:#}");
    assert!(
        tasks
            .iter()
            .any(|t| t["transcript_ref"].as_str().is_some_and(is_timestamp)),
        "{notes:#}"
    );
}
