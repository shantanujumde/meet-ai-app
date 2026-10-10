use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use audio::Channel;
use audio::wav_writer::read_header_frames;

use super::list::{Live, list_in, recover_in};
use super::root::RootPointer;
use super::view::{MeetingSummary, meeting_dir, rename_in, summarize, unparsed_lines};
use crate::error::UiError;
use crate::recording::{Phase, Status};
use crate::recording_state::RecordingState;
use store::folder_name::{prettify_slug, split_folder_name};

const AUDIO: &str = meeting_format::layout::AUDIO_DIR;
const SEGMENTS: &str = meeting_format::layout::SEGMENTS_FILE;

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
fn a_meeting_id_cannot_escape_the_meetings_root() {
    for hostile in ["..", "../../etc", "/etc/passwd", ".app", "", "a/b"] {
        assert!(
            meeting_dir(hostile).is_err(),
            "{hostile:?} must be refused before it becomes a path"
        );
    }
}

/// A meeting folder under a scratch root, loaded the way `list` and
/// `detail` load one.
fn load_fixture(name: &str, files: &[(&str, &[u8])]) -> store::folder::MeetingFolder {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("meet-ai-move-{name}-"))
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("2026-09-01-1430-platform-standup");
    fs::create_dir_all(&dir).unwrap();
    for (file, body) in files {
        fs::write(dir.join(file), body).unwrap();
    }

    store::folder::load(&dir).expect("the folder itself is readable")
}

#[test]
fn the_agent_written_title_wins_over_the_folder_slug() {
    let folder = load_fixture(
        "title",
        &[(
            "meeting.md",
            b"---\nid: 2026-09-01-1430-standup\ntitle: Platform Standup\n---\n\n## Summary\n\nShipped.\n",
        )],
    );
    let summary = summarize(&(&folder).into(), false);
    assert_eq!(summary.title, "Platform Standup");
    assert!(summary.has_analysis);
}

#[test]
fn the_attendees_in_meeting_md_reach_the_list_row() {
    let folder = load_fixture(
        "attendees",
        &[(
            "meeting.md",
            b"---\nid: 2026-09-01-1430-meeting\ntitle: Platform Standup\nattendees: [Shantanu, Priya]\n---\n",
        )],
    );
    let summary = summarize(&(&folder).into(), false);
    assert_eq!(summary.attendees, ["Shantanu", "Priya"]);
    assert!(!summary.has_analysis, "a title and attendees are not notes");

    let bare = load_fixture("no-attendees", &[]);
    assert!(summarize(&(&bare).into(), false).attendees.is_empty());
}

#[test]
fn broken_frontmatter_falls_back_to_the_slug_instead_of_failing_the_list() {
    // SPEC §7: an agent writing sloppy YAML is expected. The row still
    // shows, under the name the folder already gives it.
    let folder = load_fixture(
        "broken-title",
        &[("meeting.md", b"---\ntitle: \"unclosed\n---\n")],
    );
    let summary = summarize(&(&folder).into(), false);
    assert_eq!(summary.title, "Platform standup");
    assert!(folder.needs_attention());
}

#[test]
fn line_counts_and_the_unparsed_count_come_from_the_store_parser() {
    let folder = load_fixture(
        "counts",
        &[
            (
                "transcript.md",
                b"[00:00:04] Others: Morning.\nnot a transcript line\n[00:01:10] You: Hi.\n",
            ),
            ("notes.md", b"remember the Redis ticket"),
        ],
    );
    let summary = summarize(&(&folder).into(), false);
    assert_eq!(summary.line_count, 2);
    assert_eq!(summary.last_timestamp.as_deref(), Some("00:01:10"));
    assert!(summary.has_notes);
    assert!(!summary.has_analysis);
    assert_eq!(unparsed_lines(folder.transcript.as_ref().unwrap()), 1);
}

#[test]
fn an_empty_folder_is_a_meeting_with_nothing_in_it_yet() {
    let folder = load_fixture("empty", &[]);
    let summary = summarize(&(&folder).into(), false);
    assert_eq!(summary.line_count, 0);
    assert_eq!(summary.last_timestamp, None);
    assert!(!summary.has_notes);
    assert!(folder.transcript.is_none(), "missing, not empty");
}

#[test]
fn a_rename_reaches_the_list_row_and_a_blank_one_is_refused() {
    let root_dir = meetings_root("rename");
    let root = root_dir.path();
    let id = "2026-09-01-1430-meeting";
    meeting(root, id);

    let blank = rename_in(root, id, "  ").unwrap_err();
    assert_eq!((blank.domain.as_str(), blank.kind), ("app", "blank-title"));
    assert_eq!(summary_of(root, id, Live::Nothing).title, "Meeting");

    assert_eq!(
        rename_in(root, id, " Budget review ").unwrap(),
        "Budget review"
    );
    assert_eq!(summary_of(root, id, Live::Nothing).title, "Budget review");
}

#[test]
fn store_errors_keep_the_kinds_the_ui_already_branches_on() {
    let bad_id: UiError = store::Error::BadId("..".into()).into();
    assert_eq!(
        (bad_id.domain.as_str(), bad_id.kind),
        ("app", "bad-meeting-id")
    );
    let io: UiError = store::Error::Io(std::io::Error::other("disk gone")).into();
    assert_eq!((io.domain.as_str(), io.kind), ("app", "io"));
    assert!(io.message.contains("disk gone"), "{}", io.message);
}

#[test]
fn the_root_pointer_round_trips_through_the_file_format() {
    let written = RootPointer {
        custom_root: Some("/Volumes/Data/Meetings".into()),
    };
    let json = serde_json::to_string(&written).expect("serializes");
    assert!(json.contains("customRoot"), "{json}");
    let read: RootPointer = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(read.custom_root, written.custom_root);
}

// --- TUR-97: finished vs interrupted, and the launch-time header fix ---
//
// Every fixture is written by the recorder's own `WavWriter` and
// `SegmentsWriter`, driven through the exact sequence a real stop or a
// real kill leaves behind, so these tests break if the recorder's on-disk
// shapes change underneath the classification.

use audio::segments::{Anchor, SegmentOpen, SegmentsWriter};
use audio::wav_writer::WavWriter;

/// One second of audio at the recorder's 16 kHz.
const SECOND: u64 = 16_000;

/// A fresh, empty meetings root unique to this test.
fn meetings_root(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("meet-ai-state-{name}-"))
        .tempdir()
        .unwrap()
}

/// A meeting folder the way `recording::create_meeting_folder` makes one.
/// Returns its `audio/` folder.
fn meeting(root: &Path, id: &str) -> PathBuf {
    let dir = root.join(id);
    fs::create_dir_all(dir.join(AUDIO)).unwrap();
    fs::write(dir.join("transcript.md"), "[00:00:04] You: hello\n").unwrap();
    fs::write(dir.join("notes.md"), "").unwrap();
    dir.join(AUDIO)
}

/// A ramp, so a test can tell a sample that moved from one that did not.
fn samples(frames: u64) -> Vec<i16> {
    (0..frames).map(|n| (n % 30_000) as i16).collect()
}

/// Write one track: `checkpointed` frames synced and declared the way a
/// checkpoint or a stop does it, then `after` more appended with no header
/// patch — the samples a kill strands past the header.
fn track(audio_dir: &Path, channel: Channel, checkpointed: u64, after: u64) {
    let mut wav = WavWriter::create(&audio_dir.join(channel.wav_filename())).unwrap();
    if checkpointed > 0 {
        wav.append(&samples(checkpointed)).unwrap();
        wav.fsync_data().unwrap();
        wav.patch_header().unwrap();
    }
    if after > 0 {
        wav.append(&samples(after)).unwrap();
    }
}

/// `segments.json` for one segment, written through the recorder's own
/// atomic write. `sys_frames: None` is a tap that never started.
fn segments_json(audio_dir: &Path, mic_frames: u64, sys_frames: Option<u64>) {
    let mut writer = SegmentsWriter::new(SegmentOpen {
        start_host_ns: 1_000_000_000,
        start_continuous_ns: Some(1_000_000_000),
        start_unix_ns: None,
        mic_rate: 16_000,
        sys_rate: if sys_frames.is_some() { 16_000 } else { 0 },
        mic_device_rate: None,
        sys_device_rate: None,
        reason: audio::segments::reason::START.into(),
    });
    let sys_frames = sys_frames.unwrap_or(0);
    writer.update_frames(mic_frames, sys_frames);
    writer.checkpoint_anchor(Anchor {
        mic_host_ns: 2_000_000_000,
        mic_frames,
        sys_host_ns: 2_000_000_000,
        sys_frames,
    });
    writer.write_atomic(&audio_dir.join(SEGMENTS)).unwrap();
}

fn summary_of(root: &Path, id: &str, live: Live<'_>) -> MeetingSummary {
    list_in(root, live)
        .unwrap()
        .meetings
        .into_iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| panic!("{id} is missing from the list"))
}

fn header_frames(audio_dir: &Path, channel: Channel) -> u64 {
    read_header_frames(&audio_dir.join(channel.wav_filename())).unwrap()
}

#[test]
fn a_recording_stopped_with_the_stop_button_is_finished() {
    let root_dir = meetings_root("clean");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1129-meeting");
    // `RecordingSession::stop`: both headers finalised to every sample,
    // then `segments.json` with the same totals — A5's equality.
    track(&audio, Channel::Mic, 10 * SECOND, 0);
    track(&audio, Channel::System, 10 * SECOND + 32, 0);
    segments_json(&audio, 10 * SECOND, Some(10 * SECOND + 32));

    let summary = summary_of(root, "2026-09-30-1129-meeting", Live::Nothing);

    assert_eq!(summary.recording_state, RecordingState::Finished);
    assert_eq!(
        summary.audio_ms,
        Some(10_002),
        "the longer track, from its header"
    );
}

#[test]
fn a_v0_3_0_kill_with_zero_byte_headers_and_no_segments_json_is_interrupted() {
    // The shape on disk at ~/Meetings/2026-09-30-1140-meeting: v0.3.0
    // never checkpointed, so a kill leaves headers still declaring 0
    // bytes over the real samples, and no `segments.json` at all.
    let root_dir = meetings_root("prefix-kill");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1140-meeting");
    track(&audio, Channel::Mic, 0, 16 * SECOND);
    track(&audio, Channel::System, 0, 16 * SECOND);

    let summary = summary_of(root, "2026-09-30-1140-meeting", Live::Nothing);

    assert_eq!(summary.recording_state, RecordingState::Interrupted);
    assert_eq!(
        summary.audio_ms,
        Some(0),
        "no player can reach audio the header hides"
    );
}

#[test]
fn a_kill_between_checkpoints_is_interrupted() {
    // With 5 s checkpoints: header and segments agree at the last one,
    // and up to one checkpoint of samples sits past the header.
    let root_dir = meetings_root("checkpointed-kill");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1200-meeting");
    track(&audio, Channel::Mic, 60 * SECOND, 3 * SECOND);
    track(&audio, Channel::System, 60 * SECOND, 3 * SECOND);
    segments_json(&audio, 60 * SECOND, Some(60 * SECOND));

    let summary = summary_of(root, "2026-09-30-1200-meeting", Live::Nothing);

    assert_eq!(summary.recording_state, RecordingState::Interrupted);
    assert_eq!(summary.audio_ms, Some(60_000));
}

#[test]
fn a_kill_between_the_segments_json_rename_and_the_header_patch_is_interrupted() {
    // A5's benign crash window: the segments describe a checkpoint the
    // headers never got to declare. File length alone misses this one
    // when nothing was appended after the sync — the segment totals do
    // not.
    let root_dir = meetings_root("window-kill");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1210-meeting");
    track(&audio, Channel::Mic, 55 * SECOND, 0);
    track(&audio, Channel::System, 55 * SECOND, 0);
    segments_json(&audio, 60 * SECOND, Some(60 * SECOND));

    let summary = summary_of(root, "2026-09-30-1210-meeting", Live::Nothing);

    assert_eq!(summary.recording_state, RecordingState::Interrupted);
}

#[test]
fn the_meeting_being_recorded_is_never_labelled_interrupted() {
    let root_dir = meetings_root("live");
    let root = root_dir.path();
    let older = meeting(root, "2026-09-30-1140-meeting");
    track(&older, Channel::Mic, 0, SECOND);
    let live = meeting(root, "2026-09-30-1300-meeting");
    track(&live, Channel::Mic, 0, SECOND);
    let live_id = "2026-09-30-1300-meeting";

    assert_eq!(
        summary_of(root, live_id, Live::Meeting(live_id)).recording_state,
        RecordingState::Recording
    );
    // Being live covers one meeting, not every meeting.
    assert_eq!(
        summary_of(root, "2026-09-30-1140-meeting", Live::Meeting(live_id)).recording_state,
        RecordingState::Interrupted
    );
    // And once nobody is recording into them, the same files are what
    // they look like.
    assert_eq!(
        summary_of(root, live_id, Live::Nothing).recording_state,
        RecordingState::Interrupted
    );
}

#[test]
fn the_recorder_status_maps_onto_the_meeting_being_written() {
    let status = |phase, id: Option<&str>| Status {
        phase,
        meeting_id: id.map(str::to_string),
        started_at_ms: None,
        error: None,
        pause: Default::default(),
    };
    let id = "2026-09-30-1300-meeting";
    assert_eq!(Live::from_status(&status(Phase::Idle, None)), Live::Nothing);
    assert_eq!(
        Live::from_status(&status(Phase::Recording, Some(id))),
        Live::Meeting(id)
    );
    // `Stopping` is still writing: the headers are being finalised.
    assert_eq!(
        Live::from_status(&status(Phase::Stopping, Some(id))),
        Live::Meeting(id)
    );
    assert_eq!(
        Live::from_status(&status(Phase::Starting, None)),
        Live::Nothing
    );
}

#[test]
fn a_folder_with_no_audio_is_finished_not_interrupted() {
    let root_dir = meetings_root("no-audio");
    let root = root_dir.path();
    // No `audio/` at all — the Phase 2a folders on disk look like this.
    fs::create_dir_all(root.join("2026-09-28-1216-meeting")).unwrap();
    // An empty `audio/`, and one where retention (L16) deleted the WAVs
    // but left `segments.json`.
    meeting(root, "2026-09-28-1225-meeting");
    let retained = meeting(root, "2026-09-28-1235-meeting");
    segments_json(&retained, 60 * SECOND, Some(60 * SECOND));

    for id in [
        "2026-09-28-1216-meeting",
        "2026-09-28-1225-meeting",
        "2026-09-28-1235-meeting",
    ] {
        let summary = summary_of(root, id, Live::Nothing);
        assert_eq!(summary.recording_state, RecordingState::Finished, "{id}");
        assert_eq!(summary.audio_ms, None, "{id}");
    }
}

#[test]
fn a_clean_stop_with_no_system_audio_is_still_finished() {
    // SPEC §3.4: a tap that never started still gets a `segments.json`,
    // with `sys_rate` 0 — and there is no `system.wav` to compare.
    let root_dir = meetings_root("mic-only");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1400-meeting");
    track(&audio, Channel::Mic, 30 * SECOND, 0);
    segments_json(&audio, 30 * SECOND, None);

    assert_eq!(
        summary_of(root, "2026-09-30-1400-meeting", Live::Nothing).recording_state,
        RecordingState::Finished
    );
}

#[test]
fn one_odd_folder_never_fails_the_list() {
    let root_dir = meetings_root("odd");
    let root = root_dir.path();
    let fine = meeting(root, "2026-09-30-1129-meeting");
    track(&fine, Channel::Mic, SECOND, 0);
    track(&fine, Channel::System, SECOND, 0);
    segments_json(&fine, SECOND, Some(SECOND));

    // Not a WAV header at all, and a `segments.json` that is not JSON.
    let odd = meeting(root, "2026-09-30-1500-meeting");
    fs::write(odd.join("mic.wav"), b"RIF").unwrap();
    fs::write(odd.join(SEGMENTS), "{ not json").unwrap();
    // A stray file beside the meetings.
    fs::write(root.join("stray.txt"), "hi").unwrap();

    let list = list_in(root, Live::Nothing).expect("one odd folder must not fail the list");

    assert_eq!(list.meetings.len(), 2);
    assert_eq!(
        summary_of(root, "2026-09-30-1500-meeting", Live::Nothing).recording_state,
        RecordingState::Interrupted,
        "a meeting the app cannot show was stopped must not claim it was"
    );
    assert_eq!(
        summary_of(root, "2026-09-30-1129-meeting", Live::Nothing).recording_state,
        RecordingState::Finished
    );
}

#[test]
fn launch_recovery_makes_a_v0_3_0_kill_playable_without_moving_a_sample() {
    let root_dir = meetings_root("recover");
    let root = root_dir.path();
    let audio = meeting(root, "2026-09-30-1140-meeting");
    track(&audio, Channel::Mic, 0, 16 * SECOND);
    track(&audio, Channel::System, 0, 15 * SECOND);
    // A trailing half-frame: a kill can land mid-sample.
    let mic_path = audio.join("mic.wav");
    OpenOptions::new()
        .append(true)
        .open(&mic_path)
        .unwrap()
        .write_all(&[0x7f])
        .unwrap();
    let mic_before = fs::read(&mic_path).unwrap();
    let sys_before = fs::read(audio.join("system.wav")).unwrap();

    assert_eq!(recover_in(root, Live::Nothing), 2);

    assert_eq!(header_frames(&audio, Channel::Mic), 16 * SECOND);
    assert_eq!(header_frames(&audio, Channel::System), 15 * SECOND);
    let mic_after = fs::read(&mic_path).unwrap();
    let sys_after = fs::read(audio.join("system.wav")).unwrap();
    // Only the two size fields changed. Every sample byte, and the file
    // length, is exactly what the recorder left — odd byte included.
    assert_eq!(mic_after.len(), mic_before.len());
    assert_eq!(mic_after[44..], mic_before[44..]);
    assert_eq!(sys_after[44..], sys_before[44..]);
    assert_eq!(mic_after[0..4], mic_before[0..4]);
    assert_eq!(mic_after[8..40], mic_before[8..40]);
    let riff = u32::from_le_bytes(mic_after[4..8].try_into().unwrap());
    assert_eq!(u64::from(riff), 36 + 16 * SECOND * 2);

    // Still honest about what happened, and now says how much it kept.
    let summary = summary_of(root, "2026-09-30-1140-meeting", Live::Nothing);
    assert_eq!(summary.recording_state, RecordingState::Interrupted);
    assert_eq!(summary.audio_ms, Some(16_000));

    // Idempotent: nothing left to do, nothing touched.
    assert_eq!(recover_in(root, Live::Nothing), 0);
    assert_eq!(fs::read(&mic_path).unwrap(), mic_after);
}

#[test]
fn launch_recovery_leaves_everything_else_exactly_as_it_was() {
    let root_dir = meetings_root("recover-scope");
    let root = root_dir.path();
    // A finished meeting.
    let finished = meeting(root, "2026-09-30-1129-meeting");
    track(&finished, Channel::Mic, 5 * SECOND, 0);
    segments_json(&finished, 5 * SECOND, None);
    // A checkpointed kill: its excess samples were never in any
    // `segments.json`, and `WavWriter::open_append` discards exactly
    // those — this must not resurrect them.
    let checkpointed = meeting(root, "2026-09-30-1200-meeting");
    track(&checkpointed, Channel::Mic, 60 * SECOND, 3 * SECOND);
    segments_json(&checkpointed, 60 * SECOND, None);
    // A file whose header this app did not write.
    let foreign = meeting(root, "2026-09-30-1210-meeting");
    let mut junk = vec![0u8; 44];
    junk.extend_from_slice(&[1u8; 4000]);
    fs::write(foreign.join("mic.wav"), &junk).unwrap();
    // The meeting being recorded right now, newest.
    let live = meeting(root, "2026-09-30-1300-meeting");
    track(&live, Channel::Mic, 0, 4 * SECOND);

    let dirs = [&finished, &checkpointed, &foreign, &live];
    let snapshot = || -> Vec<Vec<u8>> {
        dirs.iter()
            .map(|dir| fs::read(dir.join("mic.wav")).unwrap())
            .collect()
    };
    let before = snapshot();

    assert_eq!(
        recover_in(root, Live::Meeting("2026-09-30-1300-meeting")),
        0
    );

    assert_eq!(before, snapshot());
}

#[test]
fn a_meeting_with_notes_switched_off_is_marked_and_not_wrapped_up() {
    // SPEC A11, TUR-12: off writes a meeting.md holding only the switch.
    // That is not a wrapped-up meeting.
    let root_dir = meetings_root("notes-off");
    let root = root_dir.path();
    let id = "2026-09-30-1129-private";
    meeting(root, id);
    let writes = store::watcher::SelfWrites::default();

    store::notes_switch::set(root, id, false, &writes).unwrap();
    let off = summary_of(root, id, Live::Nothing);
    assert!(off.notes_off);
    assert!(!off.has_analysis);
    assert_eq!(off.title, "Private");

    store::notes_switch::set(root, id, true, &writes).unwrap();
    let on = summary_of(root, id, Live::Nothing);
    assert!(!on.notes_off);
    assert!(!on.has_analysis);
}

#[test]
fn notes_written_without_analyzed_by_still_count_as_wrapped_up() {
    // The copy-prompt path: the agent writes the sections and may leave
    // `analyzed_by` out.
    let folder = load_fixture(
        "sections-only",
        &[(
            "meeting.md",
            b"---\nid: 2026-09-01-1430-standup\ntitle: Standup\nagent_notes: off\n---\n\n\
              ## Summary\n\n## Decisions\n\n- Redis.\n\n## Action Items\n\n## Open Questions\n",
        )],
    );
    let summary = summarize(&(&folder).into(), false);
    assert!(summary.has_analysis);
    assert!(summary.notes_off);
}
