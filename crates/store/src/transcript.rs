//! `transcript.md` — strict, parseable, append-only (SPEC §3.4).
//!
//! `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`, one utterance per line.
//! This module is the reader. Writing is `crates/stt`'s job: its
//! `TranscriptSink` is the only thing allowed to append to the file, so nothing
//! here opens it for anything but reading. A second writer would break "a line,
//! once written, is never rewritten" the first time the two disagreed.
//!
//! [`format_line`] is re-exported here anyway, as the pure pair of
//! [`parse_line`]. It and stt's writer both render through
//! `meeting_format::transcript`, and the parity tests below still hold the
//! file stt writes to what this module reads, to the byte.

use std::path::Path;

use crate::{Error, Problem};

/// The two speaker labels v1 can produce (L5) — the same type `stt` writes
/// with, from `meeting-format`, so store's normal build still does not depend
/// on `stt` (stt is only a dev-dependency, for the parity tests).
pub use meeting_format::Speaker;

/// Render one line, applying the §3.4 write rules: whitespace collapsed, and
/// `None` for text that is empty after collapsing. The one rendering, shared
/// with stt's sink.
pub use meeting_format::transcript::format_line;

/// One parsed line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// 0-based line number in the file, counting skipped lines.
    pub seq: usize,
    /// `HH:MM:SS` exactly as written.
    pub time: String,
    /// The same timestamp in seconds from the start of the recording.
    pub start_sec: u64,
    /// Serialized as the file's own label (`"You"`/`"Others"`), which is what
    /// the meeting view has always received — not [`Speaker`]'s lowercase wire
    /// name, which belongs to the live transcript events.
    #[serde(serialize_with = "serialize_label")]
    pub speaker: Speaker,
    /// Everything after `Speaker: `, verbatim.
    pub text: String,
}

/// A parsed transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    pub lines: Vec<Line>,
    /// `Problem::UnparsedLines` when any non-blank line did not match.
    pub problems: Vec<Problem>,
}

/// Parse one line. `None` for anything that is not the §3.4 contract,
/// including a speaker other than `You` / `Others`. Empty text after the
/// colon parses (a hand edit can produce one) with `text == ""`.
pub fn parse_line(raw: &str) -> Option<(String, u64, Speaker, String)> {
    // SPEC §3.4's regex, written out rather than compiled: the prefix is
    // fixed-width and anchored, and the regex crate is not in the workspace.
    let rest = raw.strip_prefix('[')?;
    let (time, rest) = rest.split_once("] ")?;
    let start_sec = hms_to_sec(time)?;

    // Anchored on the literal labels rather than "everything up to the first
    // colon", so a mangled speaker (`Priya:`) is reported as unparsed instead
    // of inventing a third speaker.
    let (speaker, after) = if let Some(after) = rest.strip_prefix("You:") {
        (Speaker::You, after)
    } else {
        (Speaker::Others, rest.strip_prefix("Others:")?)
    };

    // `: (.*)$` — exactly one space after the colon, then the text verbatim. A
    // bare `You:` is the empty-text case; `You:text` with no space is not the
    // contract.
    let text = if after.is_empty() {
        after
    } else {
        after.strip_prefix(' ')?
    };

    Some((time.to_string(), start_sec, speaker, text.to_string()))
}

fn serialize_label<S: serde::Serializer>(speaker: &Speaker, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_str(speaker.label())
}

/// Parse a whole file. Blank lines (including the trailing newline) are file
/// structure and are neither lines nor problems.
pub fn parse(raw: &str) -> Transcript {
    let mut transcript = Transcript::default();
    let mut unparsed = 0;
    // A BOM, which some editors add on save, would otherwise glue itself to
    // the first line's `[` and hide it. `str::lines` also drops a trailing
    // `\r`, so a file re-saved with CRLF endings (a Windows editor, a sync
    // tool) parses the same as LF.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    for (seq, raw_line) in raw.lines().enumerate() {
        if raw_line.trim().is_empty() {
            continue;
        }
        match parse_line(raw_line) {
            Some((time, start_sec, speaker, text)) => transcript.lines.push(Line {
                seq,
                time,
                start_sec,
                speaker,
                text,
            }),
            None => unparsed += 1,
        }
    }
    if unparsed > 0 {
        transcript
            .problems
            .push(Problem::UnparsedLines { count: unparsed });
    }
    transcript
}

/// Read `transcript.md`. `Ok(None)` if it does not exist — a different state
/// from "exists but empty". Non-UTF-8 content loads as an empty transcript
/// with `Problem::Unreadable`.
pub fn read(path: &Path) -> Result<Option<Transcript>, Error> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    match String::from_utf8(bytes) {
        Ok(raw) => Ok(Some(parse(&raw))),
        // SPEC §7: a file that is not text loads with a badge instead of
        // failing the meeting. Nothing is salvaged from it — a lossy decode
        // could turn a corrupted timestamp into a plausible wrong one.
        Err(error) => Ok(Some(Transcript {
            lines: Vec::new(),
            problems: vec![Problem::Unreadable {
                detail: error.to_string(),
            }],
        })),
    }
}

/// `\d{2}:\d{2}:\d{2}` and nothing longer, as seconds.
///
/// Minutes and seconds are not range-checked, because the §3.4 regex does not
/// check them either: a hand-typed `00:75:00` is still a readable line.
fn hms_to_sec(value: &str) -> Option<u64> {
    let b = value.as_bytes();
    let well_formed = b.len() == 8
        && b[2] == b':'
        && b[5] == b':'
        && [0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_digit());
    if !well_formed {
        return None;
    }
    let two = |i: usize| u64::from(b[i] - b'0') * 10 + u64::from(b[i + 1] - b'0');
    Some(two(0) * 3600 + two(3) * 60 + two(6))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A scratch folder unique to this test and this process, emptied first.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meet-ai-store-transcript-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn the_spec_3_4_example_lines_parse() {
        let (time, start_sec, speaker, text) =
            parse_line("[00:00:04] Others: Morning everyone, let's start with the API work.")
                .expect("the spec's own example must parse");
        assert_eq!(time, "00:00:04");
        assert_eq!(start_sec, 4);
        assert_eq!(speaker, Speaker::Others);
        assert_eq!(text, "Morning everyone, let's start with the API work.");

        let (_, start_sec, speaker, text) =
            parse_line("[00:00:11] You: Sessions are still in memory, that's the blocker.")
                .expect("the spec's own example must parse");
        assert_eq!(start_sec, 11);
        assert_eq!(speaker, Speaker::You);
        assert_eq!(text, "Sessions are still in memory, that's the blocker.");
    }

    #[test]
    fn the_timestamp_becomes_seconds_from_the_start() {
        let (_, start_sec, _, _) = parse_line("[01:02:03] You: hi").unwrap();
        assert_eq!(start_sec, 3723);
    }

    #[test]
    fn brackets_and_colons_inside_speech_need_no_escaping() {
        // §3.4: the prefix is fixed-width and anchored, so `(.*)$` is safe.
        let (_, _, _, text) = parse_line("[01:02:03] You: see issue [TUR-17]: it is the shell")
            .expect("punctuation in speech is not a parse failure");
        assert_eq!(text, "see issue [TUR-17]: it is the shell");
    }

    #[test]
    fn lines_that_are_not_the_contract_are_rejected() {
        for bad in [
            "",
            "just some prose",
            "## A heading",
            // A mangled speaker is unparsed, not a third speaker.
            "[00:00:04] Priya: hello",
            // Timestamp not HH:MM:SS.
            "[0:00:04] You: hello",
            "[00:00:04.5] You: hello",
            "[000:00:04] You: hello",
            // Missing the single space the regex requires.
            "[00:00:04]You: hello",
            "[00:00:04] You:hello",
        ] {
            assert!(parse_line(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn empty_text_still_parses_even_though_it_is_never_written() {
        // §3.4 forbids writing one, but a hand-edited file can contain one and
        // the reader must not fall over.
        for raw in ["[00:00:04] You:", "[00:00:04] You: "] {
            let (_, _, speaker, text) = parse_line(raw).expect("must not fail");
            assert_eq!(speaker, Speaker::You);
            assert_eq!(text, "");
        }
    }

    #[test]
    fn text_is_taken_verbatim_after_the_separator() {
        let (_, _, _, text) = parse_line("[00:00:04] Others:  two  spaces ").unwrap();
        assert_eq!(text, " two  spaces ");
    }

    #[test]
    fn seq_counts_the_lines_that_were_skipped() {
        let t = parse("not a transcript line\n\n[00:00:04] You: first real line\n");
        assert_eq!(t.lines.len(), 1);
        assert_eq!(t.lines[0].seq, 2);
    }

    #[test]
    fn unparsed_lines_are_counted_once_and_blank_lines_are_not() {
        let t = parse("# Transcript\n\n   \n[00:00:04] You: hi\nPriya: hello\n\n");
        assert_eq!(t.lines.len(), 1);
        assert_eq!(t.problems, vec![Problem::UnparsedLines { count: 2 }]);

        let clean = parse("[00:00:04] You: hi\n\n");
        assert!(clean.problems.is_empty());
    }

    #[test]
    fn crlf_line_endings_parse_like_lf() {
        let t = parse("[00:00:04] You: hi\r\n[00:00:05] Others: hey\r\n");
        assert!(t.problems.is_empty());
        assert_eq!(t.lines[0].text, "hi");
        assert_eq!(t.lines[1].text, "hey");
        assert_eq!(t.lines[1].speaker, Speaker::Others);
    }

    #[test]
    fn a_missing_file_is_none_and_an_empty_file_is_an_empty_transcript() {
        let dir = scratch("missing");
        let path = dir.join(crate::TRANSCRIPT_FILE);
        assert_eq!(read(&path).unwrap(), None);

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "").unwrap();
        assert_eq!(read(&path).unwrap(), Some(Transcript::default()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_utf8_content_loads_as_unreadable() {
        let dir = scratch("binary");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(crate::TRANSCRIPT_FILE);
        std::fs::write(&path, b"[00:00:04] You: \xff\xfe\n").unwrap();
        let t = read(&path).unwrap().expect("the file exists");
        assert!(t.lines.is_empty());
        assert!(
            matches!(t.problems.as_slice(), [Problem::Unreadable { .. }]),
            "{:?}",
            t.problems
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn whitespace_is_collapsed_before_writing() {
        assert_eq!(
            format_line(4, Speaker::You, "  one\ntwo\r\nthree\tfour    five  ").as_deref(),
            Some("[00:00:04] You: one two three four five")
        );
        assert_eq!(format_line(4, Speaker::You, "\n\t  \r"), None);
    }

    #[test]
    fn hours_past_99_mirror_stt_rather_than_being_invented() {
        // `{h:02}` is a minimum width, so 100 h renders as three digits — which
        // the §3.4 regex then rejects. That is stt's behaviour; changing it is
        // a format migration, not a fix in this crate.
        assert_eq!(
            format_line(100 * 3600, Speaker::You, "late").as_deref(),
            Some("[100:00:00] You: late")
        );
    }

    #[test]
    fn a_parsed_line_reaches_the_ui_with_the_files_capitalised_label() {
        // `Speaker` itself serializes lowercase (the live events' wire name);
        // the meeting view has always been sent the label as written.
        let t = parse("[00:00:04] You: hi\n[00:00:05] Others: hello\n");
        let json = serde_json::to_value(&t.lines).unwrap();
        assert_eq!(json[0]["speaker"], "You");
        assert_eq!(json[1]["speaker"], "Others");
        assert_eq!(json[0]["startSec"], 4);
    }

    fn to_stt(speaker: Speaker) -> stt::Speaker {
        match speaker {
            Speaker::You => stt::Speaker::You,
            Speaker::Others => stt::Speaker::Others,
        }
    }

    const CROSS_CHECK: [(u64, Speaker, &str); 4] = [
        (
            4,
            Speaker::Others,
            "Morning everyone, let's start with the API work.",
        ),
        (11, Speaker::You, "see issue [TUR-17]: it is the shell"),
        (3723, Speaker::Others, "an hour in: still going"),
        (35999, Speaker::You, "last one ] : ]"),
    ];

    #[test]
    fn format_line_matches_what_stt_writes_to_the_byte() {
        for (start_sec, speaker, text) in CROSS_CHECK {
            let ours = format_line(start_sec, speaker, text).unwrap();
            let theirs = stt::format_transcript_line(&stt::Utterance {
                start_sec,
                speaker: to_stt(speaker),
                text: stt::collapse_whitespace(text).unwrap(),
            });
            assert_eq!(ours, theirs);
            assert_eq!(speaker.label(), to_stt(speaker).label());
        }
    }

    #[test]
    fn store_reads_back_exactly_what_the_stt_sink_wrote() {
        use stt::TranscriptSink;

        let dir = scratch("sink");
        let path = dir.join(crate::TRANSCRIPT_FILE);
        let mut sink = stt::MarkdownSink::create(&path).unwrap();
        for (start_sec, speaker, text) in CROSS_CHECK {
            sink.write(&stt::Utterance {
                start_sec,
                speaker: to_stt(speaker),
                text: text.to_string(),
            })
            .unwrap();
        }
        sink.flush().unwrap();
        drop(sink);

        let t = read(&path).unwrap().expect("the sink created the file");
        assert!(t.problems.is_empty(), "{:?}", t.problems);
        let got: Vec<_> = t
            .lines
            .iter()
            .map(|l| (l.start_sec, l.speaker, l.text.as_str()))
            .collect();
        assert_eq!(got, CROSS_CHECK.to_vec());
        assert_eq!(
            t.lines.iter().map(|l| l.seq).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
