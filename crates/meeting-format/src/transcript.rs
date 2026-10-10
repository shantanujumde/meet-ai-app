//! The `transcript.md` line format (SPEC §3.4).
//!
//! `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`, one utterance per line. The
//! writer (`stt`'s sink) and the reader (`store::transcript`) used to render
//! this separately and hold each other to it with a parity test; now both call
//! [`render_line`], so there is one rendering to be right about. Reading is
//! the same: [`parse_line`] and [`parse_hms`] are the one parser, used by
//! `store` (the meeting view, search) and `prompts` (the Start Work excerpt),
//! so a line one of them shows is never one the other skips (TUR-175).
//!
//! No escaping: the prefix is fixed-width and anchored, so `]` and `:` inside
//! speech are safe.

use crate::Speaker;

/// Collapse recognized text the way SPEC §3.4 requires.
///
/// One utterance is exactly one line, so every `\n`, `\r`, `\t` and run of
/// spaces becomes a single space. Returns `None` for whitespace-only input —
/// that is the last line of defence against a hallucinated empty segment.
pub fn collapse_whitespace(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// Render one line, `[HH:MM:SS] Speaker: text`, from text that is already
/// collapsed.
///
/// Does not collapse: `stt::Utterance` carries text that is collapsed and known
/// non-empty by construction, and re-collapsing it on every write would hide a
/// producer that broke that promise. Use [`format_line`] for raw text.
pub fn render_line(start_sec: u64, speaker: Speaker, text: &str) -> String {
    let (h, m, s) = (start_sec / 3600, (start_sec % 3600) / 60, start_sec % 60);
    format!("[{h:02}:{m:02}:{s:02}] {}: {text}", speaker.label())
}

/// Render one line from raw text, applying the §3.4 write rules: whitespace
/// collapsed per [`collapse_whitespace`], and `None` for text that is empty
/// after collapsing — empty text is never written.
pub fn format_line(start_sec: u64, speaker: Speaker, text: &str) -> Option<String> {
    collapse_whitespace(text).map(|text| render_line(start_sec, speaker, &text))
}

/// One line of `transcript.md`, borrowing from the raw text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedLine<'a> {
    /// `HH:MM:SS` exactly as written.
    pub time: &'a str,
    /// The same timestamp in seconds from the start of the recording.
    pub start_sec: u64,
    pub speaker: Speaker,
    /// Everything after `Speaker: `, verbatim.
    pub text: &'a str,
}

/// Parse one line. `None` for anything that is not the §3.4 contract,
/// including a speaker other than `You` / `Others`. Empty text after the
/// colon parses (a hand edit can produce one) with `text == ""`.
pub fn parse_line(raw: &str) -> Option<ParsedLine<'_>> {
    // SPEC §3.4's regex, written out rather than compiled: the prefix is
    // fixed-width and anchored, and the regex crate is not in the workspace.
    let rest = raw.strip_prefix('[')?;
    let (time, rest) = rest.split_once("] ")?;
    let start_sec = parse_hms(time)?;

    // Anchored on the literal labels rather than "everything up to the first
    // colon", so a mangled speaker (`Priya:`) is reported as unparsed instead
    // of inventing a third speaker.
    let (speaker, after) = if let Some(after) = rest.strip_prefix("You:") {
        (Speaker::You, after)
    } else {
        (Speaker::Others, rest.strip_prefix("Others:")?)
    };

    // `: (.*)$`: exactly one space after the colon, then the text verbatim. A
    // bare `You:` is the empty-text case; `You:text` with no space is not the
    // contract.
    let text = if after.is_empty() {
        after
    } else {
        after.strip_prefix(' ')?
    };

    Some(ParsedLine {
        time,
        start_sec,
        speaker,
        text,
    })
}

/// `\d{2}:\d{2}:\d{2}` and nothing longer, as seconds.
///
/// Minutes and seconds are not range-checked, because the §3.4 regex does not
/// check them either: a hand-typed `00:75:00` is still a readable line, at
/// 75 minutes.
pub fn parse_hms(value: &str) -> Option<u64> {
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
    use super::*;

    #[test]
    fn a_rendered_line_parses_back() {
        let line = render_line(3_723, Speaker::Others, "a: [b]");
        assert_eq!(
            parse_line(&line),
            Some(ParsedLine {
                time: "01:02:03",
                start_sec: 3_723,
                speaker: Speaker::Others,
                text: "a: [b]",
            })
        );
    }

    #[test]
    fn the_timestamp_is_exactly_two_digits_each_and_not_range_checked() {
        assert_eq!(parse_hms("00:75:00"), Some(4_500));
        assert_eq!(parse_hms("00:00:99"), Some(99));
        for bad in [
            "0:00:01",
            "00:01",
            "000:00:01",
            "00:00:1",
            "aa:00:00",
            "00-00-00",
        ] {
            assert_eq!(parse_hms(bad), None, "{bad}");
        }
    }

    #[test]
    fn lines_that_are_not_the_contract_do_not_parse() {
        for bad in [
            "[00:00:01] Priya: hi",
            "[00:00:01]You: hi",
            "[00:00:01] You:hi",
            "[0:00:01] You: hi",
            "00:00:01 You: hi",
        ] {
            assert_eq!(parse_line(bad), None, "{bad}");
        }
        assert_eq!(parse_line("[00:00:01] You:").map(|l| l.text), Some(""));
    }

    #[test]
    fn a_line_is_zero_padded_hours_minutes_seconds() {
        assert_eq!(
            render_line(3_723, Speaker::Others, "a: [b]"),
            "[01:02:03] Others: a: [b]"
        );
    }

    #[test]
    fn raw_text_is_collapsed_and_empty_text_is_never_a_line() {
        assert_eq!(
            format_line(4, Speaker::You, " can\teveryone\r\nhear  me? ").as_deref(),
            Some("[00:00:04] You: can everyone hear me?")
        );
        assert_eq!(format_line(4, Speaker::You, " \n\t "), None);
    }
}
