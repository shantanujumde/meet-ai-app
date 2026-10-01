//! 3. Transcript.

use super::*;

const SPEECH: &[&str] = &[
    "hello",
    "]",
    "[",
    ":",
    ": ",
    "] ",
    "[00:00:01] You:",
    "[00:00:01] Others: nested",
    "You:",
    "Others:",
    "\t",
    "\n",
    "\r\n",
    "\r",
    "  ",
    " ",
    "é",
    "🎉",
    "中文",
    "\u{2028}",
    "\u{85}",
    "\u{a0}",
    "\u{200b}",
    "\u{feff}",
    "\u{3000}",
    "#",
    "## Summary",
    "---",
    "\\",
    "\"",
    "'",
    "\0",
];

fn random_speech(rng: &mut Rng) -> String {
    (0..rng.below(12)).map(|_| *rng.pick(SPEECH)).collect()
}

fn random_speaker(rng: &mut Rng) -> Speaker {
    if rng.one_in(2) {
        Speaker::You
    } else {
        Speaker::Others
    }
}

#[test]
fn every_formatted_line_parses_back_to_what_was_said() {
    let mut rng = Rng::new(0x5eed_0007);
    for _ in 0..5000 {
        // `{h:02}` grows to three digits past 99h, which §3.4's regex then
        // rejects — a documented limitation (see transcript.rs), so stay
        // under it.
        let start_sec = rng.below(360_000) as u64;
        let speaker = random_speaker(&mut rng);
        let text = random_speech(&mut rng);
        let want = collapsed(&text);
        let Some(line) = transcript::format_line(start_sec, speaker, &text) else {
            assert_eq!(want, "", "{text:?} was dropped");
            continue;
        };
        assert!(!line.contains('\n') && !line.contains('\r'), "{line:?}");
        let (time, sec, who, said) =
            transcript::parse_line(&line).unwrap_or_else(|| panic!("{line:?} did not parse back"));
        assert_eq!(sec, start_sec, "{line:?}");
        assert_eq!(
            time,
            format!(
                "{:02}:{:02}:{:02}",
                start_sec / 3600,
                start_sec % 3600 / 60,
                start_sec % 60
            )
        );
        assert_eq!(who, speaker, "{line:?}");
        assert_eq!(said, want, "{line:?}");
    }
}

/// Have stt's sink append `count` random lines to `path` and check they all
/// come back, in order, after whatever was there, which must be untouched.
///
/// The sink is SPEC §3.4's only writer of `transcript.md`, so this is the
/// write path the reader actually has to keep up with. Text goes through
/// `stt::collapse_whitespace` first, as the engines do before handing it over.
fn append_and_check(rng: &mut Rng, path: &Path, count: usize) {
    use stt::TranscriptSink;

    let before = fs::read(path).unwrap_or_default();
    let mut appended = Vec::new();
    let mut sink = stt::MarkdownSink::create(path).unwrap();
    for _ in 0..count {
        let start_sec = rng.below(360_000) as u64;
        let speaker = random_speaker(rng);
        let text = random_speech(rng);
        let Some(text) = stt::collapse_whitespace(&text) else {
            continue;
        };
        sink.write(&stt::Utterance {
            start_sec,
            speaker,
            text: text.clone(),
        })
        .unwrap();
        appended.push((start_sec, speaker, text));
    }
    sink.flush().unwrap();
    drop(sink);
    let after = fs::read(path).unwrap();
    assert!(
        after.starts_with(&before),
        "earlier content changed:\nbefore {:?}\nafter  {:?}",
        String::from_utf8_lossy(&before),
        String::from_utf8_lossy(&after)
    );
    let t = transcript::read(path).unwrap().unwrap();
    let tail: Vec<_> = t.lines[t.lines.len() - appended.len()..]
        .iter()
        .map(|l| (l.start_sec, l.speaker, l.text.clone()))
        .collect();
    assert_eq!(tail, appended);
    assert!(after.ends_with(b"\n"));
}

#[test]
fn lines_the_stt_sink_appends_come_back_in_order_after_whatever_was_there() {
    // Every start ends in a newline (or is empty): the sink appends without
    // adding a separator, so a hand edit that drops the final newline glues
    // the next line onto it. That is the writer's concern, not the reader's.
    let scratch = Scratch::new("append");
    let mut rng = Rng::new(0x5eed_0008);
    let starts = [
        ("missing", None),
        ("empty", Some("")),
        (
            "crlf",
            Some("[00:00:04] You: hi\r\n[00:00:05] Others: hey\r\n"),
        ),
        ("junk", Some("# Transcript\n\nnot a line\n[bad] You: x\n")),
        ("multibyte-last", Some("[00:00:04] You: 🎉\n")),
        ("only-newlines", Some("\n\n\n")),
    ];
    for (name, start) in starts {
        let path = scratch.path().join(name).join(TRANSCRIPT_FILE);
        if let Some(start) = start {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, start).unwrap();
        }
        append_and_check(&mut rng, &path, 200);
        // A second recording session opens the file again and carries on.
        append_and_check(&mut rng, &path, 50);
    }
}

#[test]
fn every_parsed_line_counts_every_line_before_it() {
    // `seq` is the 0-based line number, skipped lines included — for CRLF,
    // LF and a mix of both.
    let mut rng = Rng::new(0x5eed_0009);
    for _ in 0..300 {
        let mut raw = String::new();
        let mut expected = Vec::new();
        for i in 0..rng.below(30) {
            if rng.one_in(3) {
                raw.push_str(rng.pick(&["junk", "", "   ", "[00:00:01]You: x", "## h"]));
            } else {
                let sec = rng.below(360_000) as u64;
                raw.push_str(&transcript::format_line(sec, Speaker::You, "x").unwrap());
                expected.push((i, sec));
            }
            raw.push_str(rng.pick(&["\n", "\r\n"]));
        }
        let t = transcript::parse(&raw);
        let got: Vec<_> = t.lines.iter().map(|l| (l.seq, l.start_sec)).collect();
        assert_eq!(got, expected, "{raw:?}");
    }
}

#[test]
fn a_bom_at_the_start_of_a_transcript_does_not_hide_the_first_line() {
    // frontmatter::split tolerates a BOM; transcript::parse does not, so a
    // transcript re-saved by a Windows editor reports its first utterance
    // as unparsed.
    let t = transcript::parse("\u{feff}[00:00:04] You: hi\n[00:00:05] Others: hey\n");
    assert_eq!(t.problems, vec![], "{t:?}");
    assert_eq!(t.lines.len(), 2, "{t:?}");
}
