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
    let first = slot.show(zoom()).expect("shown");
    let second = slot.show(reminder(false)).expect("shown");
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
    assert_eq!(slot.show(update.clone()), None);
    assert_eq!(slot.current(), None);
    slot.show(zoom()).expect("shown");
    assert_eq!(
        slot.show(update.clone()).map(|shown| shown.prompt),
        Some(update)
    );
}

#[test]
fn a_timeout_closes_it_and_records_nothing() {
    let mut slot = Slot::default();
    let mut starts = Starts::default();
    let shown = slot.show(zoom()).expect("shown");
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
    let shown = slot.show(zoom()).expect("shown");
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
    let shown = slot.show(zoom()).expect("shown");
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    starts.run(slot.answer(shown.id, PopupAnswer::Record));
    starts.run(slot.answer(shown.id, PopupAnswer::JoinAndRecord));
    assert_eq!(starts.0, vec![None]);
    assert_eq!(slot.current(), None);
}

#[test]
fn a_reminder_records_its_own_event() {
    let mut slot = Slot::default();
    let shown = slot.show(reminder(true)).expect("shown");
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
    let shown = slot.show(reminder(true)).expect("shown");
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
    let shown = slot.show(test).expect("shown");
    slot.answer(shown.id, PopupAnswer::Join);
    assert_eq!(slot.current(), None, "a test's Join just closes it");
}

#[test]
fn the_popup_sits_top_right_of_the_work_area() {
    let at = window::top_right(
        PhysicalPosition::new(0, 25),
        PhysicalSize::new(1920, 1055),
        1.0,
    );
    assert_eq!(at, PhysicalPosition::new(1920 - 380 - 16, 25 + 16));
    // A second monitor to the left, at 2x.
    let at = window::top_right(
        PhysicalPosition::new(-2880, 0),
        PhysicalSize::new(2880, 1800),
        2.0,
    );
    assert_eq!(at, PhysicalPosition::new(-760 - 32, 32));
    // Narrower than the popup: pinned to the left edge.
    let at = window::top_right(
        PhysicalPosition::new(0, 0),
        PhysicalSize::new(200, 200),
        1.0,
    );
    assert_eq!(at.x, 0);
}

#[test]
fn the_window_hears_the_id_and_the_prompt() {
    let mut slot = Slot::default();
    let shown = slot.show(zoom()).expect("shown");
    let json = serde_json::to_value(&shown).expect("serialises");
    assert_eq!(json["id"], 1);
    assert_eq!(json["prompt"]["reason"], "Zoom is open.");
    let answer: PopupAnswer = serde_json::from_str("\"joinAndRecord\"").expect("parses");
    assert_eq!(answer, PopupAnswer::JoinAndRecord);
}

#[test]
fn the_card_is_compact() {
    assert_eq!((window::WIDTH, window::HEIGHT), (380.0, 72.0));
}

#[test]
fn the_card_sits_under_the_macos_menu_bar() {
    // A 1512x982 MacBook screen at 2x whose work area starts below a 37pt
    // menu bar (the shape tauri-runtime-wry gives from `visibleFrame`).
    let at = window::top_right(
        PhysicalPosition::new(0, 74),
        PhysicalSize::new(3024, 1890),
        2.0,
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
        let shown = slot.show(calendar_reminder(can_join)).expect("shown");
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
fn reminders_use_the_card_on_every_os_detection_only_where_the_platform_says() {
    let reminder = Signal::Calendar {
        title: "Standup".to_string(),
        attendees: 3,
    };
    let app = Signal::Process {
        process: detect::processes::name_of("Zoom").to_string(),
    };
    // macOS's switch (false): reminders in the card, detection as before.
    assert!(popup_for(&reminder, false));
    assert!(!popup_for(&app, false));
    assert!(!popup_for(&Signal::AudioActivity, false));
    // Windows and Linux (true): every prompt in the card.
    for signal in [&reminder, &app, &Signal::AudioActivity] {
        assert!(popup_for(signal, true), "{signal:?}");
    }
    assert!(uses_popup(&reminder), "this OS too");
    assert_eq!(uses_popup(&app), platform::DETECTION_IN_POPUP);
}
