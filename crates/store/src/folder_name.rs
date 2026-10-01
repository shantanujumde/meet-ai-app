//! Meeting folder names (SPEC §3.1): `YYYY-MM-DD-HHMM-slug`.
//!
//! The rules for building one when a recording starts, and for reading one
//! back when the meetings list is shown, live together so the two cannot
//! drift apart. Nothing here knows about the app shell or its state.

use std::path::Path;

use meeting_format::layout;

/// Build a SPEC §3.1 meeting id from an already formatted `YYYY-MM-DD-HHMM`
/// stamp: `YYYY-MM-DD-HHMM-slug`.
///
/// The slug is `meeting` until Phase 5a can name it from the calendar event.
/// A fixed slug is better than a guessed one — the list falls back to showing
/// the date and time, which is true, instead of a title nobody chose.
pub fn meeting_id(stamp: &str) -> String {
    format!("{stamp}-meeting")
}

/// Create the meeting folder and the files SPEC §3.1 says live in it.
///
/// `transcript.md` is created empty and never written to here: §3.4 makes it
/// append-only and `crates/stt`'s `TranscriptSink` is the only thing allowed to
/// append. Creating it up front means the review view can open a meeting that
/// is still recording without a missing-file branch. `audio/` is also created
/// here, ahead of `RecordingSession::start`'s own (idempotent)
/// `create_dir_all`, so folder creation stays one step even though the audio
/// inside it is now the session's to write.
///
/// Returns the id actually used. Ids only resolve to the minute, so a second
/// recording started in the same minute as the last one would land in that
/// meeting's folder — appending to its WAVs and `transcript.md` and replacing
/// its `segments.json`. It gets `<id>-2` (then `-3`, …) instead: still a §3.1
/// `YYYY-MM-DD-HHMM-slug` name, just with a longer slug. `create_dir` rather
/// than an existence check, so claiming the name is atomic.
///
/// `announce` hears each candidate id just before its folder is created, so
/// the recorder can publish it first (TUR-97: a folder whose id is not known
/// yet would read as an interrupted meeting).
pub fn create_meeting_folder(
    root: &Path,
    base: &str,
    mut announce: impl FnMut(&str),
) -> std::io::Result<String> {
    std::fs::create_dir_all(root)?;
    let mut n = 1;
    let (id, dir) = loop {
        let id = if n == 1 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        let dir = root.join(&id);
        announce(&id);
        match std::fs::create_dir(&dir) {
            Ok(()) => break (id, dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(error) => return Err(error),
        }
    };
    std::fs::create_dir_all(layout::audio_dir(&dir))?;
    for file in [layout::TRANSCRIPT_FILE, layout::NOTES_FILE] {
        let path = dir.join(file);
        if !path.exists() {
            std::fs::write(&path, "")?;
        }
    }
    Ok(id)
}

/// Split `2026-09-01-1430-standup` into date, time and slug.
///
/// Returns `None`s rather than failing for a folder someone renamed by hand.
/// The list still shows it; it just sorts to the end and shows no date.
pub fn split_folder_name(id: &str) -> (Option<String>, Option<String>, Option<String>) {
    // YYYY-MM-DD-HHMM = 4+1+2+1+2+1+4 = 15 characters before the slug.
    let parts: Vec<&str> = id.splitn(5, '-').collect();
    let [year, month, day, hhmm, rest @ ..] = parts.as_slice() else {
        return (None, None, None);
    };
    let dated = year.len() == 4
        && month.len() == 2
        && day.len() == 2
        && hhmm.len() == 4
        && [year, month, day, hhmm]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()));
    if !dated {
        return (None, None, None);
    }

    (
        Some(format!("{year}-{month}-{day}")),
        Some(format!("{}:{}", &hhmm[..2], &hhmm[2..])),
        rest.first().map(|slug| (*slug).to_string()),
    )
}

/// `platform-standup` -> `Platform standup`.
pub fn prettify_slug(slug: String) -> String {
    let spaced = slug.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_spec_3_1_folder_name() {
        let (date, time, slug) = split_folder_name("2026-09-01-1430-standup");
        assert_eq!(date.as_deref(), Some("2026-09-01"));
        assert_eq!(time.as_deref(), Some("14:30"));
        assert_eq!(slug.as_deref(), Some("standup"));
    }

    #[test]
    fn a_slug_with_dashes_survives_the_split() {
        let (_, _, slug) = split_folder_name("2026-09-01-1430-platform-standup");
        assert_eq!(slug.as_deref(), Some("platform-standup"));
        assert_eq!(prettify_slug("platform-standup".into()), "Platform standup");
    }

    #[test]
    fn a_hand_renamed_folder_is_listed_without_a_date_rather_than_dropped() {
        assert_eq!(split_folder_name("my-old-notes"), (None, None, None));
        assert_eq!(split_folder_name(""), (None, None, None));
    }

    #[test]
    fn a_meeting_id_reads_back_through_the_split() {
        let id = meeting_id("2026-09-01-1430");
        assert_eq!(id, "2026-09-01-1430-meeting");
        let (date, time, slug) = split_folder_name(&id);
        assert_eq!(date.as_deref(), Some("2026-09-01"));
        assert_eq!(time.as_deref(), Some("14:30"));
        assert_eq!(slug.as_deref(), Some("meeting"));
    }
}
