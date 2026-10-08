//! The one notification path for every detection signal (TUR-27).
//!
//! Whatever noticed the meeting — a running app, audio activity, or the
//! calendar's one-minute reminder (TUR-30, [`remind`]) — ends up here. It says
//! *why* the user is being asked (from the [`Signal`]), posts a system
//! notification, and tells the window, whose `DetectionPrompt` banner holds
//! **Record** and **Dismiss** (and **Open brief** for a reminder). Two signals
//! for the same call make one prompt ([`super::merge`]).
//! macOS notifications posted through the plugin have no action buttons, so
//! the notification itself only explains; clicking it brings meet-ai forward,
//! where the banner is waiting. Nothing here starts a recording (L15).
//!
//! TUR-78: a reminder names the meeting and how soon it starts ("starts in 2
//! min", from `detection.remind_before_minutes`), and an event with a meeting
//! link adds **Join and record** and **Join** ([`super::actions`]). A prompt
//! whose `detection` switch is off is dropped here ([`allowed`]), so a switch
//! turned off in Settings stops its prompts at once. [`test_reminder`] is the
//! card's "Send a test reminder".
//!
//! TUR-108: a reminder shows as the compact card window on every OS (title,
//! time range, one split button; `super::popup`). TUR-147: detection prompts
//! do too, on every OS, macOS included, as a narrow card that names the app
//! in one line ([`Prompt::headline`]) and offers "Never for <App>"
//! ([`Prompt::app`]). The notification stays the fallback when the card
//! cannot show ([`super::popup::uses_popup`]).

use chrono::{DateTime, Utc};
use detect::Signal;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};

use super::Detection;
use super::merge::{Delivery, Merger};
use crate::events::DETECTION_PROMPT_EVENT;
use crate::recording::{Phase, Recorder};

/// The notification's title, for every signal.
pub const TITLE: &str = "Record this meeting?";

/// What the window hears on [`DETECTION_PROMPT_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    /// What was noticed.
    pub signal: Signal,
    /// Why the user is being asked, as one sentence: "Zoom is open."
    pub reason: String,
    /// Replace the prompt on screen, if there is one, and open none: this
    /// call was already asked about (`detection/merge.rs`).
    pub update_only: bool,
    /// The reminded calendar event (TUR-78), for the banner's Join and
    /// Record. `None` for other prompts and for a test.
    pub event_id: Option<String>,
    /// The event has a meeting link: offer **Join and record** and **Join**.
    pub can_join: bool,
    /// "Send a test reminder" (TUR-78): the buttons only close the banner.
    pub test: bool,
    /// The meeting's name, for the reminder card's first line (TUR-108).
    /// `None` for a detection prompt, whose card leads with the reason.
    pub title: Option<String>,
    /// When the reminded meeting starts and ends, in Unix milliseconds, for
    /// the card's "9:00 AM to 9:30 AM". `None` when there is no event.
    /// A JS number: milliseconds since 1970 stay far below 2^53.
    #[specta(type = Option<specta_typescript::Number>)]
    pub starts_at_ms: Option<i64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub ends_at_ms: Option<i64>,
    /// The service the meeting link joins, for "Join Meet": `"meet"`,
    /// `"zoom"`, `"teams"` or `"other"` (`join_url::join_service`). `None`
    /// without a link.
    pub join_service: Option<String>,
    /// The card's one line naming what was noticed (TUR-147): "Zoom call",
    /// "Audio activity", or the meeting's title for a reminder.
    pub headline: String,
    /// The app the prompt is about, for the card's "Never for Zoom"
    /// (TUR-147; TUR-143 keeps the list). `None` when no one app explains
    /// it: audio activity, a reminder.
    pub app: Option<String>,
}

/// The card's line for `signal` ([`Prompt::headline`]).
pub fn headline(signal: &Signal) -> String {
    match signal {
        Signal::Calendar { title, .. } => title.clone(),
        Signal::Process { process } => format!("{} call", detect::processes::label(process)),
        Signal::AudioActivity => "Audio activity".to_string(),
        Signal::Call { app, browser } => super::call_start::headline(app, *browser),
    }
}

/// The app `signal` names, as the user knows it ([`Prompt::app`]).
pub fn app_name(signal: &Signal) -> Option<String> {
    match signal {
        Signal::Process { process } => Some(detect::processes::label(process).to_string()),
        Signal::Call { app, .. } => Some(app.clone()),
        Signal::Calendar { .. } | Signal::AudioActivity => None,
    }
}

/// The fake event a test reminder is about.
pub const TEST_MEETING: &str = "Test meeting";

/// The prompt for `signal`, or `None` while a recording is starting, running
/// or stopping — the user is already recording, and asking again is nagging.
/// The detection loop already holds back while recording; this is the same
/// rule for every signal, at the one place they all pass.
pub fn prompt_for(phase: Phase, signal: &Signal) -> Option<Prompt> {
    (phase == Phase::Idle).then(|| Prompt {
        signal: signal.clone(),
        reason: signal.reason(),
        update_only: false,
        event_id: None,
        can_join: false,
        test: false,
        title: match signal {
            Signal::Calendar { title, .. } => Some(title.clone()),
            Signal::Process { .. } | Signal::AudioActivity => None,
            Signal::Call { .. } => None,
        },
        starts_at_ms: None,
        ends_at_ms: None,
        join_service: None,
        headline: headline(signal),
        app: app_name(signal),
    })
}

/// Whether `signal`'s `detection` switch is on: `processes` for a meeting
/// app, `audio_activity` for the mic and speakers, `calendar` for a reminder.
pub fn allowed(config: &crate::config::DetectionConfig, signal: &Signal) -> bool {
    match signal {
        Signal::Process { .. } => config.processes,
        Signal::AudioActivity => config.audio_activity,
        Signal::Calendar { .. } => config.calendar,
        Signal::Call { .. } => config.call_start,
    }
}

/// "starts in 2 min", or "is starting now" once it has.
pub fn starts_in(start: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (start - now).num_seconds();
    if seconds <= 0 {
        return "is starting now".to_string();
    }
    format!("starts in {} min", (seconds + 59) / 60)
}

/// The reminder for `event` at `now`, or `None` while recording (see
/// [`prompt_for`]).
pub fn reminder_prompt(
    phase: Phase,
    event: &::calendar::Event,
    now: DateTime<Utc>,
) -> Option<Prompt> {
    let signal = Signal::Calendar {
        title: event.title.clone(),
        attendees: event.attendees,
    };
    let mut prompt = prompt_for(phase, &signal)?;
    prompt.reason = format!(
        "“{}” {}, with {} people invited.",
        event.title,
        starts_in(event.start, now),
        event.attendees
    );
    prompt.event_id = Some(event.id.clone());
    let link = super::actions::join_link(event);
    prompt.can_join = link.is_some();
    prompt.join_service = link
        .and_then(::calendar::join_url::join_service)
        .map(str::to_string);
    prompt.starts_at_ms = Some(event.start.timestamp_millis());
    prompt.ends_at_ms = Some(event.end.timestamp_millis());
    Some(prompt)
}

/// "Send a test reminder": a fake half-hour meeting on Google Meet starting
/// in `minutes` from `now`, laid out like a reminder with a link, that never
/// records.
pub fn test_prompt(phase: Phase, minutes: u32, now: DateTime<Utc>) -> Option<Prompt> {
    let signal = Signal::Calendar {
        title: TEST_MEETING.to_string(),
        attendees: 0,
    };
    let mut prompt = prompt_for(phase, &signal)?;
    let when = if minutes == 0 {
        "is starting now".to_string()
    } else {
        format!("starts in {minutes} min")
    };
    prompt.reason =
        format!("“{TEST_MEETING}” {when}. This is a test reminder: nothing will be recorded.");
    prompt.can_join = true;
    prompt.test = true;
    let starts = now + chrono::Duration::minutes(i64::from(minutes));
    prompt.starts_at_ms = Some(starts.timestamp_millis());
    prompt.ends_at_ms = Some((starts + chrono::Duration::minutes(30)).timestamp_millis());
    prompt.join_service = Some("meet".to_string());
    Some(prompt)
}

/// The notification's body: the reason, and where the buttons are.
fn body(prompt: &Prompt) -> String {
    match prompt.signal {
        Signal::Calendar { .. } if prompt.can_join => format!(
            "{} Open meet-ai to join, record it or read the brief.",
            prompt.reason
        ),
        Signal::Calendar { .. } => format!(
            "{} Open meet-ai to record it or read the brief.",
            prompt.reason
        ),
        Signal::Process { .. } | Signal::AudioActivity => {
            format!("{} Open meet-ai to record it.", prompt.reason)
        }
        Signal::Call { .. } => format!("{}. Open meet-ai to record it.", prompt.headline),
    }
}

/// Is a recording starting, running or stopping right now?
pub fn recording(app: &AppHandle) -> bool {
    current_phase(app) != Phase::Idle
}

fn current_phase(app: &AppHandle) -> Phase {
    app.try_state::<Recorder>()
        .map_or(Phase::Idle, |recorder| recorder.status().phase)
}

/// Ask the user whether to record, because of `signal` (a meeting app or
/// audio activity). Dropped if a reminder just asked about this call.
pub fn notify(app: &AppHandle, signal: &Signal) {
    if !switched_on(app, signal) {
        return;
    }
    deliver(app, prompt_for(current_phase(app), signal), Merger::other);
}

/// Remind the user that `event` starts soon (TUR-30, TUR-78), and ask
/// whether to join and record it.
pub fn remind(app: &AppHandle, event: &::calendar::Event) {
    let prompt = reminder_prompt(current_phase(app), event, Utc::now());
    if !prompt
        .as_ref()
        .is_none_or(|prompt| switched_on(app, &prompt.signal))
    {
        return;
    }
    if prompt.is_some()
        && let Some(detection) = app.try_state::<Detection>()
    {
        detection.remember_reminded(event.clone());
    }
    let ends = event.end;
    deliver(app, prompt, move |merger, now| merger.reminder(now, ends));
}

/// "Send a test reminder" (TUR-78): the reminder for a fake meeting
/// starting in `minutes`, past the merge so it never hides a real prompt.
/// `false` while recording, when nothing is asked.
pub fn test_reminder(app: &AppHandle, minutes: u32) -> bool {
    let prompt = test_prompt(current_phase(app), minutes, Utc::now());
    let sent = prompt.is_some();
    deliver(app, prompt, |_, _| Delivery::New);
    sent
}

/// Is `signal`'s switch on now? Logged when it is not.
fn switched_on(app: &AppHandle, signal: &Signal) -> bool {
    let on = app
        .try_state::<Detection>()
        .is_none_or(|detection| allowed(&detection.config(), signal));
    if !on {
        tracing::debug!(
            ?signal,
            "that prompt is switched off in Settings; not asking"
        );
    }
    on
}

/// The one path every prompt takes: not while recording (`prompt` is
/// `None`), merged with a recent prompt for the same call, then a
/// notification and the banner.
fn deliver(
    app: &AppHandle,
    prompt: Option<Prompt>,
    merge: impl FnOnce(&mut Merger, DateTime<Utc>) -> Delivery,
) {
    use tauri_plugin_notification::NotificationExt as _;

    let Some(mut prompt) = prompt else {
        tracing::debug!("a meeting signal while recording; not asking");
        return;
    };
    let signal = &prompt.signal.clone();
    let delivery = app
        .try_state::<Detection>()
        .map_or(Delivery::New, |detection| {
            detection.merge(|merger| merge(merger, Utc::now()))
        });
    match delivery {
        Delivery::Drop => {
            tracing::debug!(?signal, "already asked about this call; not asking again");
            return;
        }
        Delivery::UpdateOnly => prompt.update_only = true,
        Delivery::New => {}
    }
    tracing::info!(
        ?signal,
        ?delivery,
        "a meeting looks like it started; asking whether to record"
    );
    // TUR-59, TUR-108, TUR-147: every prompt, on every OS, shows as the card
    // window with the buttons on it; the notification is only the fallback
    // when it can't show.
    let in_popup = super::popup::uses_popup(signal) && super::popup::show(app, &prompt);
    if delivery == Delivery::New
        && !in_popup
        && let Err(error) = app
            .notification()
            .builder()
            .title(TITLE)
            .body(body(&prompt))
            .show()
    {
        // The banner below still asks, whenever the window is looked at.
        tracing::warn!(%error, "could not show the detection notification");
    }
    if let Err(error) = app.emit(DETECTION_PROMPT_EVENT, &prompt) {
        tracing::warn!(%error, "could not send the detection prompt to the window");
    }
    // TUR-76: the banner is in the window, which may be hidden in the menu bar.
    // The popup asks without bringing the window forward.
    if delivery == Delivery::New && !in_popup {
        crate::lifecycle::reveal_for_prompt(app);
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn zoom() -> Signal {
        Signal::Process {
            process: detect::processes::name_of("Zoom").to_string(),
        }
    }

    #[test]
    fn an_idle_recorder_is_asked_and_told_why() {
        let prompt = prompt_for(Phase::Idle, &zoom()).expect("asks when idle");
        assert_eq!(prompt.signal, zoom());
        assert_eq!(prompt.reason, "Zoom is open.");
        assert_eq!(body(&prompt), "Zoom is open. Open meet-ai to record it.");
    }

    #[test]
    fn no_prompt_while_recording() {
        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            assert_eq!(prompt_for(phase, &zoom()), None, "{phase:?}");
            assert_eq!(prompt_for(phase, &Signal::AudioActivity), None, "{phase:?}");
        }
    }

    #[test]
    fn every_signal_takes_the_same_path() {
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        for signal in [zoom(), calendar, Signal::AudioActivity] {
            let prompt = prompt_for(Phase::Idle, &signal).expect("asks");
            assert_eq!(prompt.reason, signal.reason());
        }
    }

    #[test]
    fn the_window_gets_the_signal_and_the_reason() {
        let prompt = prompt_for(Phase::Idle, &zoom()).expect("asks");
        assert_eq!(
            serde_json::to_value(&prompt).expect("serialises"),
            serde_json::json!({
                "signal": { "kind": "process", "process": detect::processes::name_of("Zoom") },
                "reason": "Zoom is open.",
                "updateOnly": false,
                "eventId": null,
                "canJoin": false,
                "test": false,
                "title": null,
                "startsAtMs": null,
                "endsAtMs": null,
                "joinService": null,
                "headline": "Zoom call",
                "app": "Zoom"
            })
        );
    }

    #[test]
    fn the_card_names_the_app_in_one_line() {
        let zoom = prompt_for(Phase::Idle, &zoom()).expect("asks");
        assert_eq!(zoom.headline, "Zoom call");
        assert_eq!(zoom.app.as_deref(), Some("Zoom"));
        let audio = prompt_for(Phase::Idle, &Signal::AudioActivity).expect("asks");
        assert_eq!(audio.headline, "Audio activity");
        assert_eq!(audio.app, None, "no one app to never ask about");
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        let reminder = prompt_for(Phase::Idle, &calendar).expect("asks");
        assert_eq!(reminder.headline, "Standup");
        assert_eq!(reminder.app, None);
    }

    fn whatsapp() -> Signal {
        Signal::Call {
            app: "WhatsApp".to_string(),
            browser: false,
        }
    }

    #[test]
    fn a_call_prompt_names_the_app_and_offers_never_for_it() {
        let prompt = prompt_for(Phase::Idle, &whatsapp()).expect("asks when idle");
        assert_eq!(prompt.headline, "WhatsApp call detected");
        assert_eq!(prompt.app.as_deref(), Some("WhatsApp"));
        assert_eq!(prompt.reason, "WhatsApp is using your microphone.");
        assert_eq!(prompt.title, None);
        assert_eq!(
            body(&prompt),
            "WhatsApp call detected. Open meet-ai to record it."
        );
        let chrome = Signal::Call {
            app: "Google Chrome".to_string(),
            browser: true,
        };
        let prompt = prompt_for(Phase::Idle, &chrome).expect("asks");
        assert_eq!(prompt.headline, "Call detected in Google Chrome");
        assert_eq!(prompt.app.as_deref(), Some("Google Chrome"));
        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            assert_eq!(prompt_for(phase, &whatsapp()), None, "{phase:?}");
        }
    }

    #[test]
    fn the_call_start_switch_turns_call_prompts_off() {
        use crate::config::DetectionConfig;
        let on = DetectionConfig::default();
        assert!(allowed(&on, &whatsapp()));
        let off = DetectionConfig {
            call_start: false,
            ..on
        };
        assert!(!allowed(&off, &whatsapp()));
        assert!(allowed(&off, &zoom()));
        let no_apps = DetectionConfig {
            processes: false,
            ..on
        };
        assert!(allowed(&no_apps, &whatsapp()), "its own switch");
    }

    #[test]
    fn a_reminder_points_at_the_brief_too() {
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        let prompt = prompt_for(Phase::Idle, &calendar).expect("asks");
        assert_eq!(
            body(&prompt),
            "“Standup” starts in a minute, with 3 people invited. \
             Open meet-ai to record it or read the brief."
        );
    }

    fn event(join_url: Option<&str>) -> ::calendar::Event {
        let start = Utc.with_ymd_and_hms(2026, 10, 5, 10, 0, 0).unwrap();
        ::calendar::Event {
            id: "standup-1".to_string(),
            title: "Standup".to_string(),
            start,
            end: start + chrono::Duration::minutes(30),
            attendees: 3,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: join_url.map(str::to_string),
        }
    }

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, h, m, s).unwrap()
    }

    #[test]
    fn a_reminder_names_the_meeting_and_how_soon_it_starts() {
        let prompt =
            reminder_prompt(Phase::Idle, &event(None), at(9, 58, 0)).expect("asks when idle");
        assert_eq!(
            prompt.reason,
            "“Standup” starts in 2 min, with 3 people invited."
        );
        assert_eq!(prompt.event_id.as_deref(), Some("standup-1"));
        assert!(!prompt.can_join);
        assert!(!prompt.test);
        assert_eq!(
            body(&prompt),
            "“Standup” starts in 2 min, with 3 people invited. \
             Open meet-ai to record it or read the brief."
        );
        assert_eq!(
            reminder_prompt(Phase::Recording, &event(None), at(9, 58, 0)),
            None
        );
    }

    #[test]
    fn a_reminder_with_a_link_offers_join() {
        let prompt = reminder_prompt(
            Phase::Idle,
            &event(Some("https://zoom.us/j/1")),
            at(9, 59, 0),
        )
        .expect("asks");
        assert!(prompt.can_join);
        assert_eq!(prompt.join_service.as_deref(), Some("zoom"));
        assert_eq!(prompt.title.as_deref(), Some("Standup"));
        assert_eq!(
            prompt.starts_at_ms,
            Some(at(10, 0, 0).timestamp_millis()),
            "the card's time range"
        );
        assert_eq!(prompt.ends_at_ms, Some(at(10, 30, 0).timestamp_millis()));
        assert_eq!(
            body(&prompt),
            "“Standup” starts in 1 min, with 3 people invited. \
             Open meet-ai to join, record it or read the brief."
        );
        // Only a safe video-call link is a link to join.
        let odd = reminder_prompt(
            Phase::Idle,
            &event(Some("javascript:alert(1)")),
            at(9, 59, 0),
        )
        .expect("asks");
        assert!(!odd.can_join);
        assert_eq!(odd.join_service, None);
    }

    #[test]
    fn a_reminder_without_a_link_still_has_its_title_and_times() {
        let prompt = reminder_prompt(Phase::Idle, &event(None), at(9, 59, 0)).expect("asks");
        assert_eq!(prompt.title.as_deref(), Some("Standup"));
        assert_eq!(prompt.join_service, None);
        assert!(prompt.starts_at_ms.is_some() && prompt.ends_at_ms.is_some());
        let detection = prompt_for(Phase::Idle, &zoom()).expect("asks");
        assert_eq!(
            detection.title, None,
            "a detection card leads with its reason"
        );
        assert_eq!(detection.starts_at_ms, None);
    }

    #[test]
    fn starts_in_rounds_up_to_the_minute_and_says_now_at_the_start() {
        assert_eq!(starts_in(at(10, 0, 0), at(9, 50, 0)), "starts in 10 min");
        assert_eq!(starts_in(at(10, 0, 0), at(9, 55, 5)), "starts in 5 min");
        assert_eq!(starts_in(at(10, 0, 0), at(9, 59, 50)), "starts in 1 min");
        assert_eq!(starts_in(at(10, 0, 0), at(10, 0, 0)), "is starting now");
        assert_eq!(starts_in(at(10, 0, 0), at(10, 0, 10)), "is starting now");
    }

    #[test]
    fn each_switch_turns_its_prompt_off() {
        use crate::config::DetectionConfig;
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        let on = DetectionConfig::default();
        for signal in [zoom(), Signal::AudioActivity, calendar.clone()] {
            assert!(allowed(&on, &signal), "{signal:?}");
        }
        let no_apps = DetectionConfig {
            processes: false,
            ..on
        };
        assert!(!allowed(&no_apps, &zoom()));
        assert!(allowed(&no_apps, &Signal::AudioActivity));
        assert!(allowed(&no_apps, &calendar));
        let no_audio = DetectionConfig {
            audio_activity: false,
            ..on
        };
        assert!(!allowed(&no_audio, &Signal::AudioActivity));
        assert!(allowed(&no_audio, &zoom()));
        let no_reminders = DetectionConfig {
            calendar: false,
            ..on
        };
        assert!(!allowed(&no_reminders, &calendar));
        assert!(allowed(&no_reminders, &zoom()));
    }

    #[test]
    fn a_test_reminder_is_flagged_and_never_names_a_real_event() {
        let prompt = test_prompt(Phase::Idle, 2, at(9, 58, 0)).expect("asks when idle");
        assert!(prompt.test);
        assert!(prompt.can_join, "laid out like a reminder with a link");
        assert_eq!(prompt.event_id, None, "nothing to join or record");
        assert_eq!(prompt.title.as_deref(), Some(TEST_MEETING));
        assert_eq!(prompt.join_service.as_deref(), Some("meet"));
        assert_eq!(prompt.starts_at_ms, Some(at(10, 0, 0).timestamp_millis()));
        assert_eq!(prompt.ends_at_ms, Some(at(10, 30, 0).timestamp_millis()));
        assert_eq!(
            prompt.reason,
            "“Test meeting” starts in 2 min. This is a test reminder: nothing will be recorded."
        );
        assert!(
            test_prompt(Phase::Idle, 0, at(9, 58, 0))
                .expect("asks")
                .reason
                .contains("is starting now")
        );
        assert_eq!(test_prompt(Phase::Recording, 2, at(9, 58, 0)), None);
    }
}
