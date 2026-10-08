use detect::Signal;
use tauri::{PhysicalPosition, PhysicalSize};

use super::*;
use crate::recording::Phase;

fn zoom() -> Prompt {
    super::super::notify::prompt_for(
        Phase::Idle,
        &Signal::Process {
            process: detect::processes::name_of("Zoom").to_string(),
        },
    )
    .expect("asks when idle")
}

fn reminder(can_join: bool) -> Prompt {
    Prompt {
        event_id: Some("standup-1".to_string()),
        can_join,
        ..zoom()
    }
}

/// A fake for the app's start path: counts what the popup asked it to do.
#[derive(Default)]
struct Starts(Vec<Option<String>>);

impl Starts {
    fn run(&mut self, action: Option<Action>) {
        if let Some(Action::Start { event_id, .. }) = action {
            self.0.push(event_id);
        }
    }
}

#[test]
fn a_new_prompt_replaces_the_old_one() {
    let mut slot = Slot::default();
    let first = slot.show(zoom().into(), 0, None).expect("shown");
    let second = slot.show(reminder(false).into(), 0, None).expect("shown");
    assert_ne!(first.id, second.id);
    assert_eq!(slot.current(), Some(second));
    // The old prompt's buttons and timer do nothing now.
    assert_eq!(slot.answer(first.id, PopupAnswer::Record), None);
    assert!(!slot.expire(first.id));
    assert!(slot.current().is_some());
}

#[test]
fn an_update_only_prompt_never_opens_one() {
    let mut slot = Slot::default();
    let update = Prompt {
        update_only: true,
        ..zoom()
    };
    assert_eq!(slot.show(update.clone().into(), 0, None), None);
    assert_eq!(slot.current(), None);
    slot.show(zoom().into(), 0, None).expect("shown");
    assert_eq!(
        slot.show(update.clone().into(), 0, None)
            .map(|shown| shown.card),
        Some(Card::from(update))
    );
}

#[test]
fn a_timeout_closes_it_and_records_nothing() {
    let mut slot = Slot::default();
    let mut starts = Starts::default();
    let shown = slot.show(zoom().into(), 0, None).expect("shown");
    assert!(slot.expire(shown.id));
    assert_eq!(slot.current(), None);
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    assert!(
        starts.0.is_empty(),
        "a click after the timeout starts nothing"
    );
    assert!(AUTO_HIDE.as_secs() > 0);
}

#[test]
fn dismiss_closes_it_and_records_nothing() {
    let mut slot = Slot::default();
    let mut starts = Starts::default();
    let shown = slot.show(zoom().into(), 0, None).expect("shown");
    let action = slot.answer(shown.id, PopupAnswer::Dismiss);
    assert_eq!(action, Some(Action::Nothing));
    starts.run(action);
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    assert!(starts.0.is_empty());
    assert_eq!(slot.current(), None);
}

#[test]
fn record_starts_exactly_once() {
    let mut slot = Slot::default();
    let mut starts = Starts::default();
    let shown = slot.show(zoom().into(), 0, None).expect("shown");
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    starts.run(slot.answer(shown.id, PopupAnswer::JoinAndRecord));
    assert_eq!(starts.0, vec![None]);
    assert_eq!(slot.current(), None);
}

#[test]
fn a_reminder_records_its_own_event() {
    let mut slot = Slot::default();
    let shown = slot.show(reminder(true).into(), 0, None).expect("shown");
    assert_eq!(
        slot.answer(shown.id, PopupAnswer::JoinAndRecord),
        Some(Action::Start {
            event_id: Some("standup-1".to_string()),
            join: true
        })
    );
}

#[test]
fn join_keeps_it_up_and_records_nothing() {
    let mut slot = Slot::default();
    let shown = slot.show(reminder(true).into(), 0, None).expect("shown");
    assert_eq!(
        slot.answer(shown.id, PopupAnswer::Join),
        Some(Action::Join {
            event_id: "standup-1".to_string()
        })
    );
    assert!(slot.current().is_some());
}

#[test]
fn without_a_link_join_does_nothing() {
    for prompt in [reminder(false), zoom()] {
        assert_eq!(action_for(&prompt, PopupAnswer::Join), Action::Nothing);
        assert_eq!(
            action_for(&prompt, PopupAnswer::JoinAndRecord),
            Action::Nothing
        );
    }
}

#[test]
fn a_test_reminder_never_records_or_joins() {
    let test = Prompt {
        test: true,
        ..reminder(true)
    };
    for answer in [
        PopupAnswer::Record,
        PopupAnswer::JoinAndRecord,
        PopupAnswer::Join,
        PopupAnswer::OpenBrief,
        PopupAnswer::Dismiss,
    ] {
        assert_eq!(action_for(&test, answer), Action::Nothing, "{answer:?}");
    }
    let mut slot = Slot::default();
    let shown = slot.show(test.into(), 0, None).expect("shown");
    slot.answer(shown.id, PopupAnswer::Join);
    assert_eq!(slot.current(), None, "a test's Join just closes it");
}

#[test]
fn the_popup_sits_top_right_of_the_work_area() {
    let at = window::top_right(
        PhysicalPosition::new(0, 25),
        PhysicalSize::new(1920, 1055),
        1.0,
        window::WIDTH,
        0.0,
    );
    assert_eq!(at, PhysicalPosition::new(1920 - 380 - 16, 25 + 16));
    // A second monitor to the left, at 2x.
    let at = window::top_right(
        PhysicalPosition::new(-2880, 0),
        PhysicalSize::new(2880, 1800),
        2.0,
        window::WIDTH,
        0.0,
    );
    assert_eq!(at, PhysicalPosition::new(-760 - 32, 32));
    // Narrower than the popup: pinned to the left edge.
    let at = window::top_right(
        PhysicalPosition::new(0, 0),
        PhysicalSize::new(200, 200),
        1.0,
        window::WIDTH,
        0.0,
    );
    assert_eq!(at.x, 0);
}

#[test]
fn the_window_hears_the_id_and_the_prompt() {
    let mut slot = Slot::default();
    let shown = slot.show(zoom().into(), 0, None).expect("shown");
    let json = serde_json::to_value(&shown).expect("serialises");
    assert_eq!(json["id"], 1);
    assert_eq!(json["card"]["kind"], "prompt");
    assert_eq!(json["card"]["prompt"]["reason"], "Zoom is open.");
    assert_eq!(json["card"]["prompt"]["headline"], "Zoom call");
    assert_eq!(json["closesAtMs"], 0);
    let answer: PopupAnswer = serde_json::from_str("\"joinAndRecord\"").expect("parses");
    assert_eq!(answer, PopupAnswer::JoinAndRecord);
}

#[test]
fn the_reminder_card_is_compact_and_the_detection_card_narrow() {
    assert_eq!((window::WIDTH, window::HEIGHT), (380.0, 72.0));
    assert_eq!(
        window::Layout::of(&calendar_reminder(true).into()),
        window::Layout::Reminder
    );
    assert_eq!(window::Layout::of(&zoom().into()), window::Layout::Narrow);
    assert_eq!(window::Layout::of(&ended(10)), window::Layout::Narrow);
    // The reminder fills its window, as in TUR-108; the narrow card gets
    // room for its shadow where the OS draws none.
    assert_eq!(window::Layout::Reminder.window_size(8.0), (380.0, 72.0));
    assert_eq!(window::Layout::Narrow.window_size(0.0), (220.0, 120.0));
    assert_eq!(window::Layout::Narrow.window_size(8.0), (236.0, 136.0));
    assert_eq!(window::Layout::Reminder.inset(8.0), 0.0);
}

#[test]
fn the_narrow_card_sits_the_same_margin_from_the_corner_with_or_without_room_for_a_shadow() {
    let area = (PhysicalPosition::new(0, 25), PhysicalSize::new(1920, 1055));
    let bare = window::top_right(area.0, area.1, 1.0, 220.0, 0.0);
    assert_eq!(bare, PhysicalPosition::new(1920 - 220 - 16, 25 + 16));
    // A 236px window whose card sits 8px inside: the card's edge is still
    // 16px from the work area's.
    let inset = window::top_right(area.0, area.1, 1.0, 236.0, 8.0);
    assert_eq!(inset, PhysicalPosition::new(1920 - 236 - 8, 25 + 8));
}

#[test]
fn the_card_sits_under_the_macos_menu_bar() {
    // A 1512x982 MacBook screen at 2x whose work area starts below a 37pt
    // menu bar (the shape tauri-runtime-wry gives from `visibleFrame`).
    let at = window::top_right(
        PhysicalPosition::new(0, 74),
        PhysicalSize::new(3024, 1890),
        2.0,
        window::WIDTH,
        0.0,
    );
    assert_eq!(at, PhysicalPosition::new(3024 - 760 - 32, 74 + 32));
}

fn calendar_reminder(can_join: bool) -> Prompt {
    let event = ::calendar::Event {
        id: "standup-1".to_string(),
        title: "Standup".to_string(),
        start: chrono::Utc::now(),
        end: chrono::Utc::now() + chrono::Duration::minutes(30),
        attendees: 3,
        attendee_names: Vec::new(),
        ical_uid: None,
        join_url: can_join.then(|| "https://meet.google.com/abc-defg-hij".to_string()),
    };
    super::super::notify::reminder_prompt(Phase::Idle, &event, chrono::Utc::now())
        .expect("asks when idle")
}

#[test]
fn open_brief_closes_it_and_opens_the_meetings_brief() {
    for can_join in [true, false] {
        let mut slot = Slot::default();
        let shown = slot
            .show(calendar_reminder(can_join).into(), 0, None)
            .expect("shown");
        assert_eq!(
            slot.answer(shown.id, PopupAnswer::OpenBrief),
            Some(Action::OpenBrief {
                title: "Standup".to_string()
            })
        );
        assert_eq!(slot.current(), None);
    }
}

#[test]
fn open_brief_on_a_detection_prompt_or_a_test_does_nothing() {
    assert_eq!(action_for(&zoom(), PopupAnswer::OpenBrief), Action::Nothing);
    let test = Prompt {
        test: true,
        ..calendar_reminder(true)
    };
    assert_eq!(action_for(&test, PopupAnswer::OpenBrief), Action::Nothing);
}

#[test]
fn a_reminder_with_a_meet_link_names_the_service() {
    let prompt = calendar_reminder(true);
    assert_eq!(prompt.join_service.as_deref(), Some("meet"));
    assert_eq!(
        action_for(&prompt, PopupAnswer::JoinAndRecord),
        Action::Start {
            event_id: Some("standup-1".to_string()),
            join: true
        }
    );
    let answer: PopupAnswer = serde_json::from_str("\"openBrief\"").expect("parses");
    assert_eq!(answer, PopupAnswer::OpenBrief);
}

#[test]
fn every_prompt_uses_the_card_on_every_os() {
    let reminder = Signal::Calendar {
        title: "Standup".to_string(),
        attendees: 3,
    };
    let app = Signal::Process {
        process: detect::processes::name_of("Zoom").to_string(),
    };
    for signal in [&reminder, &app, &Signal::AudioActivity] {
        assert!(uses_popup(signal), "{signal:?}");
    }
}

fn ended(seconds: u32) -> Card {
    Card::Countdown {
        line: "Zoom call ended".to_string(),
        seconds,
    }
}

#[test]
fn never_for_names_the_app_on_screen() {
    let mut slot = Slot::default();
    let mut starts = Starts::default();
    let shown = slot.show(zoom().into(), 0, None).expect("shown");
    let action = slot.answer(shown.id, PopupAnswer::NeverFor);
    assert_eq!(
        action,
        Some(Action::NeverFor {
            app: "Zoom".to_string()
        })
    );
    starts.run(action);
    assert!(starts.0.is_empty(), "never records");
    assert_eq!(slot.current(), None, "and closes the card");
    let answer: PopupAnswer = serde_json::from_str("\"neverFor\"").expect("parses");
    assert_eq!(answer, PopupAnswer::NeverFor);
}

#[test]
fn never_for_without_an_app_or_on_a_test_does_nothing() {
    let audio = super::super::notify::prompt_for(Phase::Idle, &Signal::AudioActivity)
        .expect("asks when idle");
    assert_eq!(action_for(&audio, PopupAnswer::NeverFor), Action::Nothing);
    let test = Prompt {
        test: true,
        ..zoom()
    };
    assert_eq!(action_for(&test, PopupAnswer::NeverFor), Action::Nothing);
    for answer in [PopupAnswer::StopNow, PopupAnswer::KeepRecording] {
        assert_eq!(action_for(&zoom(), answer), Action::Nothing, "{answer:?}");
    }
}

#[test]
fn the_window_hears_a_countdown() {
    let mut slot = Slot::default();
    let shown = slot.show(ended(10), 1_000, None).expect("shown");
    let json = serde_json::to_value(&shown).expect("serialises");
    assert_eq!(
        json,
        serde_json::json!({
            "id": 1,
            "card": { "kind": "countdown", "line": "Zoom call ended", "seconds": 10 },
            "closesAtMs": 1000
        })
    );
    for (raw, answer) in [
        ("\"stopNow\"", PopupAnswer::StopNow),
        ("\"keepRecording\"", PopupAnswer::KeepRecording),
    ] {
        assert_eq!(
            serde_json::from_str::<PopupAnswer>(raw).expect("parses"),
            answer
        );
    }
}

#[test]
fn each_countdown_button_reports_itself_and_starts_nothing() {
    for (answer, end) in [
        (PopupAnswer::StopNow, CountdownEnd::StopNow),
        (PopupAnswer::KeepRecording, CountdownEnd::KeepRecording),
        (PopupAnswer::Dismiss, CountdownEnd::Closed),
        (PopupAnswer::Record, CountdownEnd::Closed),
    ] {
        let mut slot = Slot::default();
        let (tell, ends) = std::sync::mpsc::channel();
        let shown = slot.show(ended(10), 0, Some(tell)).expect("shown");
        assert_eq!(
            slot.answer(shown.id, answer),
            Some(Action::Nothing),
            "{answer:?}"
        );
        assert_eq!(ends.try_recv(), Ok(end), "{answer:?}");
        assert_eq!(slot.current(), None);
        // A second click reports nothing more.
        assert_eq!(slot.answer(shown.id, answer), None);
        assert!(ends.try_recv().is_err());
    }
}

#[test]
fn a_countdown_that_runs_out_times_out_and_only_that_and_stop_now_stop() {
    let mut slot = Slot::default();
    let (tell, ends) = std::sync::mpsc::channel();
    let shown = slot.show(ended(10), 0, Some(tell)).expect("shown");
    assert!(slot.expire(shown.id));
    assert_eq!(ends.try_recv(), Ok(CountdownEnd::TimedOut));
    assert!(!slot.expire(shown.id), "only once");
    assert!(CountdownEnd::TimedOut.stops());
    assert!(CountdownEnd::StopNow.stops());
    assert!(!CountdownEnd::KeepRecording.stops());
    assert!(!CountdownEnd::Closed.stops());
}

#[test]
fn a_cancelled_or_replaced_countdown_closes_quietly() {
    let mut slot = Slot::default();
    let (tell, ends) = std::sync::mpsc::channel();
    let shown = slot.show(ended(10), 0, Some(tell)).expect("shown");
    assert!(slot.close_if(shown.id));
    assert_eq!(ends.try_recv(), Ok(CountdownEnd::Closed));

    let (tell, ends) = std::sync::mpsc::channel();
    let first = slot.show(ended(10), 0, Some(tell)).expect("shown");
    let second = slot.show(zoom().into(), 0, None).expect("shown");
    assert_eq!(ends.try_recv(), Ok(CountdownEnd::Closed), "replaced");
    // The old countdown's timer does nothing to the new card.
    assert!(!slot.expire(first.id));
    assert_eq!(slot.current().map(|now| now.id), Some(second.id));
}

#[test]
fn a_countdown_answer_maps_every_button() {
    assert_eq!(
        countdown::end_for(PopupAnswer::StopNow),
        CountdownEnd::StopNow
    );
    assert_eq!(
        countdown::end_for(PopupAnswer::KeepRecording),
        CountdownEnd::KeepRecording
    );
    for answer in [
        PopupAnswer::Record,
        PopupAnswer::JoinAndRecord,
        PopupAnswer::Join,
        PopupAnswer::OpenBrief,
        PopupAnswer::Dismiss,
        PopupAnswer::NeverFor,
    ] {
        assert_eq!(
            countdown::end_for(answer),
            CountdownEnd::Closed,
            "{answer:?}"
        );
    }
}
