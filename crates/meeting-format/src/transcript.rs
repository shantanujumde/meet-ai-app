//! The `transcript.md` line format (SPEC §3.4).
//!
//! `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`, one utterance per line. The
//! writer (`stt`'s sink) and the reader (`store::transcript`) used to render
//! this separately and hold each other to it with a parity test; now both call
//! [`render_line`], so there is one rendering to be right about.
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

#[cfg(test)]
mod tests {
    use super::*;

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
