//! Putting a finished `transcript.md` in time order (SPEC A19, TUR-103).
//!
//! Live, both tracks append to one file through one sink, each line as soon
//! as it settles. A line settles some seconds after it was said, and the two
//! engines do not settle at the same pace, so a mic line can land after an
//! Others line that started later. The file is right line by line and wrong
//! in order.
//!
//! [`sort_by_time`] fixes that once, when the recording has stopped and both
//! tracks are done writing. It is the one place outside `stt`'s sink that
//! writes `transcript.md`, and it only reorders: every line comes back byte
//! for byte, ties keep the order they were written in (as the batch merge in
//! `stt::transcribe` does), and a file that is already in order is not
//! touched at all.

use std::path::Path;

use crate::Error;
use crate::transcript::parse_line;

/// What [`sort_by_time`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorted {
    /// There is no `transcript.md`.
    Missing,
    /// Already in time order, so the file was left as it was.
    InOrder,
    /// Rewritten in time order.
    Rewritten,
    /// Left as it was because `unparsed` lines are not §3.4 lines (a hand
    /// edit, a blank line, a half-written last line) or the file is not
    /// UTF-8 (`unparsed == 0`). Without a time for every line there is no
    /// order to put them in that cannot move one away from its context.
    LeftAlone { unparsed: usize },
}

/// Rewrite the `transcript.md` at `path` in time order, if it is not already.
///
/// Call only once nothing else can append to it: the write replaces the file
/// through a temp file and a rename (see [`crate::write_atomic`]), so a line
/// appended between the read and the rename would be lost. A reader never
/// sees a half-written file.
pub fn sort_by_time(path: &Path) -> Result<Sorted, Error> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Sorted::Missing),
        Err(error) => return Err(error.into()),
    };
    let Ok(raw) = String::from_utf8(bytes) else {
        return Ok(Sorted::LeftAlone { unparsed: 0 });
    };
    match in_time_order(&raw) {
        Order::AlreadySorted => Ok(Sorted::InOrder),
        Order::Unparsed(unparsed) => Ok(Sorted::LeftAlone { unparsed }),
        Order::Sorted(body) => {
            crate::write_atomic(path, &body)?;
            Ok(Sorted::Rewritten)
        }
    }
}

/// [`sort_by_time`] on the text alone.
#[derive(Debug, PartialEq, Eq)]
enum Order {
    AlreadySorted,
    /// How many lines did not parse, blank ones included.
    Unparsed(usize),
    /// The same lines in time order, newline-terminated.
    Sorted(String),
}

fn in_time_order(raw: &str) -> Order {
    // A BOM is kept where it was, in front of whichever line is now first.
    let (bom, body) = match raw.strip_prefix('\u{feff}') {
        Some(body) => ("\u{feff}", body),
        None => ("", raw),
    };
    let mut lines = Vec::new();
    let mut unparsed = 0;
    // `split_terminator`, not `lines`: a CRLF line keeps its `\r` and goes
    // back out exactly as it came in.
    for piece in body.split_terminator('\n') {
        let line = piece.strip_suffix('\r').unwrap_or(piece);
        match parse_line(line) {
            Some((_, start_sec, _, _)) => lines.push((start_sec, piece)),
            None => unparsed += 1,
        }
    }
    if unparsed > 0 {
        return Order::Unparsed(unparsed);
    }
    if lines.is_sorted_by_key(|(start_sec, _)| *start_sec) {
        return Order::AlreadySorted;
    }
    // Stable: lines that start in the same second keep their written order.
    lines.sort_by_key(|(start_sec, _)| *start_sec);
    let mut sorted = String::with_capacity(raw.len() + 1);
    sorted.push_str(bom);
    for (_, piece) in lines {
        sorted.push_str(piece);
        sorted.push('\n');
    }
    Order::Sorted(sorted)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn scratch() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix("meet-ai-store-transcript-order-")
            .tempdir()
            .unwrap();
        let path = dir.path().join(crate::TRANSCRIPT_FILE);
        (dir, path)
    }

    /// Write `body` and backdate it, so a test can tell whether it was
    /// rewritten.
    fn seed(path: &Path, body: &str) -> SystemTime {
        std::fs::write(path, body).unwrap();
        let then = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(then)
            .unwrap();
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn modified(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn lines_written_out_of_order_are_put_in_time_order() {
        let (_dir, path) = scratch();
        seed(
            &path,
            "[00:00:04] Others: Morning.\n\
             [00:00:02] You: Hi all.\n\
             [00:01:10] You: Sessions are the blocker.\n\
             [00:00:58] Others: What is blocking it?\n",
        );
        assert_eq!(sort_by_time(&path).unwrap(), Sorted::Rewritten);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[00:00:02] You: Hi all.\n\
             [00:00:04] Others: Morning.\n\
             [00:00:58] Others: What is blocking it?\n\
             [00:01:10] You: Sessions are the blocker.\n"
        );
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "no temp file left behind: {names:?}");
    }

    #[test]
    fn lines_in_the_same_second_keep_their_written_order() {
        let (_dir, path) = scratch();
        seed(
            &path,
            "[00:00:09] You: later\n\
             [00:00:05] Others: first at five\n\
             [00:00:05] You: second at five\n\
             [00:00:05] Others: third at five\n",
        );
        assert_eq!(sort_by_time(&path).unwrap(), Sorted::Rewritten);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[00:00:05] Others: first at five\n\
             [00:00:05] You: second at five\n\
             [00:00:05] Others: third at five\n\
             [00:00:09] You: later\n"
        );
    }

    #[test]
    fn a_file_already_in_order_is_not_written() {
        let (_dir, path) = scratch();
        let body = "[00:00:04] Others: Morning.\n\
                    [00:00:04] You: Hi.\n\
                    [00:00:11] You: Sessions are the blocker.\n";
        let before = seed(&path, body);
        assert_eq!(sort_by_time(&path).unwrap(), Sorted::InOrder);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        assert_eq!(modified(&path), before, "the file was rewritten anyway");

        let before = seed(&path, "");
        assert_eq!(sort_by_time(&path).unwrap(), Sorted::InOrder);
        assert_eq!(modified(&path), before);
    }

    #[test]
    fn a_line_that_does_not_parse_leaves_the_whole_file_alone() {
        let (_dir, path) = scratch();
        for body in [
            "[00:00:09] You: later\nPriya: a hand edit\n[00:00:05] Others: earlier\n",
            "[00:00:09] You: later\n\n[00:00:05] Others: earlier\n",
            // 100 hours renders as three digits, which §3.4 does not parse.
            "[100:00:00] You: late\n[00:00:05] Others: earlier\n",
        ] {
            let before = seed(&path, body);
            assert!(
                matches!(
                    sort_by_time(&path).unwrap(),
                    Sorted::LeftAlone { unparsed: 1 }
                ),
                "{body:?}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
            assert_eq!(modified(&path), before, "{body:?} was rewritten");
        }
    }

    #[test]
    fn a_file_that_is_not_utf8_is_left_alone() {
        let (_dir, path) = scratch();
        let body = b"[00:00:09] You: \xff\n[00:00:05] Others: earlier\n";
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            sort_by_time(&path).unwrap(),
            Sorted::LeftAlone { unparsed: 0 }
        );
        assert_eq!(std::fs::read(&path).unwrap(), body);
    }

    #[test]
    fn a_missing_file_is_reported_not_created() {
        let (_dir, path) = scratch();
        assert_eq!(sort_by_time(&path).unwrap(), Sorted::Missing);
        assert!(!path.exists());
    }

    #[test]
    fn crlf_endings_a_bom_and_a_missing_final_newline_survive() {
        assert_eq!(
            in_time_order("\u{feff}[00:00:09] You: later\r\n[00:00:05] Others: earlier"),
            Order::Sorted("\u{feff}[00:00:05] Others: earlier\n[00:00:09] You: later\r\n".into())
        );
    }

    #[test]
    fn the_sorted_file_still_reads_as_the_same_lines() {
        let raw = "[00:00:09] You: see [TUR-17]: it\n[00:00:05] Others:  two  spaces \n";
        let Order::Sorted(sorted) = in_time_order(raw) else {
            panic!("out of order input must sort");
        };
        let key = |t: crate::transcript::Transcript| {
            let mut lines: Vec<_> = t
                .lines
                .into_iter()
                .map(|l| (l.start_sec, l.speaker.label(), l.text))
                .collect();
            lines.sort();
            lines
        };
        let after = crate::transcript::parse(&sorted);
        assert!(after.problems.is_empty());
        assert_eq!(key(after), key(crate::transcript::parse(raw)));
    }
}
