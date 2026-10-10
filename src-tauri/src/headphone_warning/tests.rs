//! The headphone warning's tracking, without the app or a device (TUR-65).

use super::*;

fn warning(meeting_id: &str, show: bool) -> HeadphoneWarning {
    HeadphoneWarning {
        meeting_id: meeting_id.to_owned(),
        show,
    }
}

#[test]
fn a_recording_start_starts_one_watch() {
    let mut tracker = Tracker::new();
    assert_eq!(tracker.recorder(None), Change::Unchanged);
    assert_eq!(
        tracker.recorder(Some("m1".into())),
        Change::Started("m1".into())
    );
    // The recorder repeats its status; still one watch.
    assert_eq!(tracker.recorder(Some("m1".into())), Change::Unchanged);
}

#[test]
fn speakers_send_the_banner_once_and_a_change_sends_again() {
    let mut tracker = Tracker::new();
    tracker.recorder(Some("m1".into()));
    assert_eq!(tracker.current(), None, "not read yet");
    assert_eq!(tracker.reading("m1", true), Step::Send(warning("m1", true)));
    assert_eq!(tracker.reading("m1", true), Step::Quiet);
    assert_eq!(tracker.current(), Some(warning("m1", true)));
    // Headphones plugged in.
    assert_eq!(
        tracker.reading("m1", false),
        Step::Send(warning("m1", false))
    );
    assert_eq!(tracker.reading("m1", false), Step::Quiet);
    // And out again.
    assert_eq!(tracker.reading("m1", true), Step::Send(warning("m1", true)));
}

#[test]
fn headphones_from_the_start_send_no_banner() {
    let mut tracker = Tracker::new();
    tracker.recorder(Some("m1".into()));
    assert_eq!(
        tracker.reading("m1", false),
        Step::Send(warning("m1", false))
    );
    assert_eq!(tracker.current(), Some(warning("m1", false)));
}

#[test]
fn stopping_ends_the_watch_and_clears_a_shown_banner() {
    let mut tracker = Tracker::new();
    tracker.recorder(Some("m1".into()));
    tracker.reading("m1", true);
    assert_eq!(
        tracker.recorder(None),
        Change::Stopped(Some(warning("m1", false)))
    );
    assert_eq!(tracker.reading("m1", true), Step::Stop);
    assert_eq!(tracker.current(), None);

    // Nothing was shown: nothing to clear.
    tracker.recorder(Some("m2".into()));
    tracker.reading("m2", false);
    assert_eq!(tracker.recorder(None), Change::Stopped(None));
}

#[test]
fn an_old_watch_stops_when_a_new_recording_starts() {
    let mut tracker = Tracker::new();
    tracker.recorder(Some("m1".into()));
    tracker.recorder(None);
    tracker.recorder(Some("m2".into()));
    assert_eq!(tracker.reading("m1", true), Step::Stop);
    assert_eq!(tracker.reading("m2", true), Step::Send(warning("m2", true)));
}

#[test]
fn only_the_recording_phase_counts() {
    let id = || Some("m1".to_owned());
    assert_eq!(recording_meeting(Phase::Recording, id()), id());
    for phase in [Phase::Idle, Phase::Starting, Phase::Stopping] {
        assert_eq!(recording_meeting(phase, id()), None, "{phase:?}");
    }
    assert_eq!(recording_meeting(Phase::Recording, None), None);
}

#[test]
fn the_payload_is_camel_case() {
    assert_eq!(
        serde_json::to_value(warning("m1", true)).unwrap(),
        serde_json::json!({ "meetingId": "m1", "show": true })
    );
}

/// TUR-170: the setting is re-read every poll. Switched off mid-recording,
/// the banner goes at the next poll and the output is no longer read;
/// switched on again, it comes back. The watch ends with the recording.
#[test]
fn switching_the_setting_mid_recording_takes_effect_at_the_next_poll() {
    use std::cell::{Cell, RefCell};

    let tracker = Mutex::new(Tracker::new());
    lock_or_recover(&tracker).recorder(Some("m1".into()));

    // One entry per poll: the setting then. The recording stops after the last.
    let settings = [true, false, false, true];
    let poll = Cell::new(0);
    let reads = Cell::new(0);
    let sent = RefCell::new(Vec::new());

    watch_with(
        &tracker,
        "m1",
        || settings.get(poll.get()).copied().unwrap_or(false),
        || {
            reads.set(reads.get() + 1);
            OutputKind::Speakers
        },
        |warning| sent.borrow_mut().push(warning.clone()),
        || {
            poll.set(poll.get() + 1);
            if poll.get() == settings.len() {
                lock_or_recover(&tracker).recorder(None);
            }
        },
    );

    assert_eq!(
        *sent.borrow(),
        vec![
            warning("m1", true),
            warning("m1", false),
            warning("m1", true)
        ],
    );
    assert_eq!(
        reads.get(),
        2,
        "the output is read only while the setting is on"
    );
}
