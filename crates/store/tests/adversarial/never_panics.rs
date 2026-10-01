//! 1. Never panics.

use super::*;

/// Run every parser (and every render that is allowed) over `raw`.
fn exercise_parsers(raw: &str) {
    let _ = frontmatter::split(raw);
    if let Ok(doc) = frontmatter::parse(raw) {
        let _ = frontmatter::render(&doc);
    }
    let meeting = Meeting::parse(raw);
    let _ = meeting.render();
    for heading in SECTIONS {
        let _ = meeting.section(heading);
    }
    let _ = (meeting.id(), meeting.title(), meeting.date());
    let _ = (meeting.duration_sec(), meeting.attendees());
    let ticket = Ticket::parse(raw);
    let _ = ticket.render();
    let _ = (ticket.id(), ticket.status(), ticket.transcript_ref());
    let _ = transcript::parse(raw);
    for line in raw.split('\n') {
        let _ = transcript::parse_line(line);
    }
}

#[test]
fn no_parser_panics_on_thousands_of_generated_inputs() {
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_0001);
    for case in 0..3000 {
        let raw = generated(&mut rng, &seeds);
        let result = panic::catch_unwind(AssertUnwindSafe(|| exercise_parsers(&raw)));
        assert!(result.is_ok(), "case {case} panicked on input {raw:?}");
    }
}

#[test]
fn no_parser_panics_on_any_pair_of_pieces() {
    // Exhaustive over the small space the generator samples from.
    for a in PIECES {
        for b in PIECES {
            for raw in [format!("{a}{b}"), format!("---\n{a}{b}\n---\n{b}{a}")] {
                let result = panic::catch_unwind(|| exercise_parsers(&raw));
                assert!(result.is_ok(), "panicked on input {raw:?}");
            }
        }
    }
}

#[test]
fn multibyte_characters_next_to_every_delimiter_never_split_a_char() {
    // A str slice mid-char panics. Put a multi-byte char on each side of
    // every byte the parsers look for.
    let delimiters = [
        "---", "\n", "\r\n", "\r", "## ", "[", "] ", ":", ": ", "You:", "Others:",
    ];
    let wide = ["é", "🎉", "中", "\u{feff}", "\u{2028}"];
    for d in delimiters {
        for w in wide {
            for raw in [
                format!("{w}{d}{w}"),
                format!("---\n{w}{d}{w}\n---\n{w}{d}{w}"),
                format!("[{w}0:00:01] You: {w}{d}"),
                format!("[00:00:0{w}] You:{w}"),
                format!("[00:00:01]{w} You: x"),
                format!("[00{w}00{w}01] You: x"),
                format!("[{w}] You: x"),
                format!("## {w}{d}"),
            ] {
                let result = panic::catch_unwind(|| exercise_parsers(&raw));
                assert!(result.is_ok(), "panicked on input {raw:?}");
            }
        }
    }
}
