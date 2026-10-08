//! The call-end rules with fake readings and a fake clock: one reading per
//! [`STEP`] (the app's 2 s), as the loop takes them.

use super::*;

const STEP: Duration = Duration::from_secs(2);

fn zoom() -> MicUser {
    MicUser::new("us.zoom.xos", "Zoom", MicKind::CallApp)
}

fn chrome() -> MicUser {
    MicUser::new("com.google.Chrome", "Google Chrome", MicKind::Browser)
}

fn whatsapp() -> MicUser {
    MicUser::new("net.whatsapp.WhatsApp", "WhatsApp", MicKind::CallApp)
}

fn voice_memos() -> MicUser {
    MicUser::new("com.apple.VoiceMemos", "Voice Memos", MicKind::Other)
}

fn on(users: &[MicUser]) -> MicReading {
    MicReading::Supported(users.to_vec())
}

fn nobody() -> MicReading {
    MicReading::Supported(Vec::new())
}

/// A recording, fed one reading per [`STEP`] from a fixed start.
struct Run {
    rules: CallEnd,
    start: Instant,
    n: u32,
}

impl Run {
    fn new() -> Self {
        Self {
            rules: CallEnd::new(),
            start: Instant::now(),
            n: 0,
        }
    }

    fn now(&self) -> Instant {
        self.start + STEP * self.n
    }

    /// One reading while recording, the clock moving one step after it.
    fn read(&mut self, reading: &MicReading) -> Step {
        self.read_while(reading, true)
    }

    fn read_while(&mut self, reading: &MicReading, recording: bool) -> Step {
        let now = self.now();
        self.n += 1;
        self.rules.observe(reading, recording, now)
    }

    /// `n` readings of `reading`; every step that was not [`Step::Nothing`].
    fn reads(&mut self, reading: &MicReading, n: usize) -> Vec<Step> {
        (0..n)
            .map(|_| self.read(reading))
            .filter(|step| *step != Step::Nothing)
            .collect()
    }

    /// Readings of `nobody()` until the card goes up; its id and line.
    fn until_shown(&mut self) -> (u32, String) {
        for _ in 0..20 {
            if let Step::Show { id, line, seconds } = self.read(&nobody()) {
                assert_eq!(seconds, COUNTDOWN_SECONDS);
                return (id, line);
            }
        }
        panic!("the countdown never showed");
    }
}

/// Readings off the mic it takes for the wait to run out: the first one
/// starts it, and the card shows on the first reading [`OFF_WAIT`] later.
fn off_readings_to_show() -> usize {
    (OFF_WAIT.as_secs().div_ceil(STEP.as_secs())) as usize + 1
}

#[test]
fn the_wait_is_five_seconds_and_the_countdown_ten() {
    assert_eq!(OFF_WAIT, Duration::from_secs(5));
    assert_eq!(COUNTDOWN_SECONDS, 10);
}

#[test]
fn hanging_up_shows_the_countdown_after_five_seconds_off_the_mic() {
    let mut run = Run::new();
    assert!(run.reads(&on(&[zoom()]), 30).is_empty());
    assert_eq!(run.rules.tracked(), Some(&zoom()));
    // Just short of 5 s off: nothing yet.
    let short = run.reads(&nobody(), off_readings_to_show() - 1);
    assert!(short.is_empty(), "{short:?}");
    assert_eq!(
        run.read(&nobody()),
        Step::Show {
            id: 1,
            line: "Zoom call ended".to_string(),
            seconds: COUNTDOWN_SECONDS
        }
    );
    assert_eq!(run.rules.counting(), Some(1));
    // More readings with the app still gone show nothing more.
    assert!(run.reads(&nobody(), 3).is_empty());
}

#[test]
fn the_wait_is_measured_by_the_clock_not_the_readings() {
    let mut rules = CallEnd::new();
    let start = Instant::now();
    rules.observe(&on(&[zoom()]), true, start);
    let off = start + Duration::from_secs(10);
    assert_eq!(rules.observe(&nobody(), true, off), Step::Nothing);
    assert_eq!(
        rules.observe(&nobody(), true, off + OFF_WAIT - Duration::from_millis(1)),
        Step::Nothing
    );
    assert!(matches!(
        rules.observe(&nobody(), true, off + OFF_WAIT),
        Step::Show { .. }
    ));
}

#[test]
fn reaching_zero_stops() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::TimedOut), Step::Stop);
    // Nothing more for this recording, whatever comes next.
    assert_eq!(run.rules.tick(true, run.now()), Step::Nothing);
    assert!(run.reads(&nobody(), 10).is_empty());
    assert!(run.reads(&on(&[zoom()]), 3).is_empty());
    assert!(run.reads(&nobody(), 10).is_empty());
}

#[test]
fn stop_now_stops() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::StopNow), Step::Stop);
    assert_eq!(run.rules.counting(), None);
}

#[test]
fn the_clock_stops_at_zero_if_the_card_never_says() {
    let mut rules = CallEnd::new();
    let start = Instant::now();
    rules.observe(&on(&[zoom()]), true, start);
    rules.observe(&nobody(), true, start + STEP);
    let shown_at = start + STEP + OFF_WAIT;
    assert!(matches!(
        rules.observe(&nobody(), true, shown_at),
        Step::Show { .. }
    ));
    let zero = shown_at + Duration::from_secs(u64::from(COUNTDOWN_SECONDS));
    // 250 ms ticks: nothing until zero and the grace have passed.
    let mut at = shown_at;
    while at < zero + TIMEOUT_GRACE {
        assert_eq!(rules.tick(true, at), Step::Nothing, "{:?}", at - shown_at);
        at += Duration::from_millis(250);
    }
    assert_eq!(rules.tick(true, zero + TIMEOUT_GRACE), Step::Stop);
    assert_eq!(rules.tick(true, zero + TIMEOUT_GRACE), Step::Nothing);
}

#[test]
fn a_short_mic_drop_never_shows_the_card() {
    // Switching mics or a Bluetooth headset reconnecting: off for one or
    // two readings (under 5 s), then back.
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 5);
    for off in 1..off_readings_to_show() {
        assert!(run.reads(&nobody(), off).is_empty(), "off {off} readings");
        assert!(run.reads(&on(&[zoom()]), 2).is_empty());
    }
    // Many short drops in a row never add up.
    for _ in 0..20 {
        assert!(run.reads(&nobody(), 2).is_empty());
        assert!(run.reads(&on(&[zoom()]), 1).is_empty());
    }
}

#[test]
fn back_on_the_mic_during_the_countdown_cancels_it_and_keeps_recording() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.read(&on(&[zoom()])), Step::Cancel { id });
    assert_eq!(run.rules.counting(), None);
    // The card's own "closed" for the cancelled countdown changes nothing.
    assert_eq!(run.rules.answer(id, Answer::Closed), Step::Nothing);
    assert_eq!(run.rules.answer(id, Answer::TimedOut), Step::Nothing);
    // The same recording, still following Zoom: the next hang-up asks again.
    assert!(run.reads(&on(&[zoom()]), 5).is_empty());
    let (next, line) = run.until_shown();
    assert_eq!((next, line.as_str()), (id + 1, "Zoom call ended"));
}

#[test]
fn keep_recording_asks_again_only_after_the_app_starts_and_stops_again() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::KeepRecording), Step::Nothing);
    assert_eq!(run.rules.counting(), None);
    // Still off the mic for a long time: never asks again.
    assert!(run.reads(&nobody(), 300).is_empty());
    // Zoom back on the mic, and off again: asks.
    assert!(run.reads(&on(&[zoom()]), 2).is_empty());
    let (next, _) = run.until_shown();
    assert_eq!(next, id + 1);
}

#[test]
fn a_card_closed_without_an_answer_keeps_recording_like_keep() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::Closed), Step::Nothing);
    assert!(run.reads(&nobody(), 100).is_empty());
}

#[test]
fn stopping_by_hand_during_the_countdown_closes_the_card() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    // The countdown's own tick sees it first (every 250 ms)...
    assert_eq!(run.rules.tick(false, run.now()), Step::Close { id });
    assert_eq!(run.rules.counting(), None);
    // ...and the card's late "closed" is stale.
    assert_eq!(run.rules.answer(id, Answer::Closed), Step::Nothing);

    // Or the reading does.
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.read_while(&nobody(), false), Step::Close { id });
}

#[test]
fn stopping_by_hand_outside_a_countdown_does_nothing() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    assert_eq!(run.read_while(&on(&[zoom()]), false), Step::Nothing);
    assert_eq!(run.rules.tick(false, run.now()), Step::Nothing);
}

#[test]
fn a_manual_recording_follows_the_first_call_app_seen() {
    let mut run = Run::new();
    // Nothing on the mic, then a dictation tool, then the call.
    assert!(run.reads(&nobody(), 5).is_empty());
    assert!(run.reads(&on(&[voice_memos()]), 5).is_empty());
    assert_eq!(run.rules.tracked(), None);
    assert!(run.reads(&on(&[voice_memos(), whatsapp()]), 5).is_empty());
    assert_eq!(run.rules.tracked(), Some(&whatsapp()));
    // Zoom joining later does not take over.
    assert!(run.reads(&on(&[whatsapp(), zoom()]), 5).is_empty());
    assert_eq!(run.rules.tracked(), Some(&whatsapp()));
    // WhatsApp hangs up while Zoom stays: WhatsApp's call ended.
    run.read(&on(&[zoom()]));
    let shown = run.reads(&on(&[zoom()]), off_readings_to_show());
    assert!(
        matches!(&shown[..], [Step::Show { line, .. }] if line == "WhatsApp call ended"),
        "{shown:?}"
    );
}

#[test]
fn a_browser_call_is_followed_and_named_as_the_browser() {
    let mut run = Run::new();
    run.reads(&on(&[chrome()]), 5);
    let (_, line) = run.until_shown();
    assert_eq!(line, "Call ended in Google Chrome");
}

#[test]
fn no_call_app_seen_means_no_end_prompt() {
    let mut run = Run::new();
    assert!(run.reads(&on(&[voice_memos()]), 10).is_empty());
    assert!(run.reads(&nobody(), 100).is_empty());
    assert_eq!(run.rules.tracked(), None);
}

#[test]
fn an_os_that_cannot_list_the_apps_never_prompts() {
    let mut run = Run::new();
    run.rules.started_from_prompt("Zoom", run.now());
    assert!(run.reads(&MicReading::NotSupported, 300).is_empty());
    assert_eq!(run.rules.tracked(), None);
}

#[test]
fn a_reading_that_cannot_say_changes_nothing() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    // Failed reads mid-call never count as off the mic.
    assert!(run.reads(&MicReading::NotSupported, 30).is_empty());
    assert!(run.reads(&on(&[zoom()]), 1).is_empty());
    // Nor do they cut a wait short or long: off, unknown, off.
    run.read(&nobody());
    run.read(&MicReading::NotSupported);
    let rest = run.reads(&nobody(), off_readings_to_show());
    assert!(matches!(rest[..], [Step::Show { .. }]), "{rest:?}");
}

#[test]
fn the_app_must_be_seen_on_the_mic_before_its_absence_counts() {
    let mut run = Run::new();
    run.rules.started_from_prompt("Zoom", run.now());
    // Zoom is not on the mic from the start (the call already ended): wait.
    assert!(run.reads(&nobody(), 100).is_empty());
    assert!(run.reads(&on(&[zoom()]), 2).is_empty());
    let (_, line) = run.until_shown();
    assert_eq!(line, "Zoom call ended");
}

#[test]
fn a_recording_from_a_call_prompt_follows_that_app() {
    let mut run = Run::new();
    // Record pressed on "WhatsApp call" before the loop saw the recording.
    run.rules.started_from_prompt("WhatsApp", run.now());
    // Chrome was on the mic first, then WhatsApp: WhatsApp is followed.
    run.reads(&on(&[chrome(), whatsapp()]), 3);
    assert_eq!(run.rules.tracked(), Some(&whatsapp()));
    // Chrome leaving the mic is not the end of this call.
    assert!(run.reads(&on(&[whatsapp()]), 20).is_empty());
    let (_, line) = run.until_shown();
    assert_eq!(line, "WhatsApp call ended");
}

#[test]
fn a_prompt_after_the_loop_saw_the_recording_still_names_the_app() {
    let mut run = Run::new();
    run.reads(&on(&[chrome(), zoom()]), 2);
    assert_eq!(run.rules.tracked(), Some(&chrome()));
    run.rules.started_from_prompt("us.zoom.xos", run.now());
    assert_eq!(run.rules.tracked(), Some(&zoom()));
    assert!(run.reads(&on(&[zoom()]), 10).is_empty());
    let (_, line) = run.until_shown();
    assert_eq!(line, "Zoom call ended");
}

#[test]
fn a_prompt_named_app_of_any_kind_is_followed() {
    let mut run = Run::new();
    run.rules.started_from_prompt("voice memos", run.now());
    run.reads(&on(&[voice_memos()]), 3);
    assert_eq!(run.rules.tracked(), Some(&voice_memos()));
}

#[test]
fn a_stale_prompt_does_not_name_the_next_recording() {
    let mut rules = CallEnd::new();
    let start = Instant::now();
    // Record pressed, but the start failed: no recording followed.
    rules.started_from_prompt("WhatsApp", start);
    let later = start + ORIGIN_TTL + Duration::from_secs(1);
    rules.observe(&on(&[zoom(), whatsapp()]), true, later);
    assert_eq!(rules.tracked(), Some(&zoom()));
}

#[test]
fn each_recording_starts_over() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::KeepRecording), Step::Nothing);
    // Stopped by hand, then a new recording with WhatsApp.
    run.read_while(&nobody(), false);
    assert_eq!(run.rules.tracked(), None);
    run.reads(&on(&[whatsapp()]), 3);
    assert_eq!(run.rules.tracked(), Some(&whatsapp()));
    let (_, line) = run.until_shown();
    assert_eq!(line, "WhatsApp call ended");
}

#[test]
fn a_stop_by_the_countdown_resets_once_the_recording_ends() {
    let mut run = Run::new();
    run.reads(&on(&[zoom()]), 3);
    let (id, _) = run.until_shown();
    assert_eq!(run.rules.answer(id, Answer::TimedOut), Step::Stop);
    // Stopping takes a few readings; then the next recording asks again.
    assert!(run.reads(&nobody(), 2).is_empty());
    run.read_while(&nobody(), false);
    run.reads(&on(&[zoom()]), 3);
    assert!(matches!(run.until_shown(), (n, _) if n == id + 1));
}

#[test]
fn not_recording_never_prompts() {
    let mut run = Run::new();
    for _ in 0..50 {
        assert_eq!(run.read_while(&on(&[zoom()]), false), Step::Nothing);
        assert_eq!(run.read_while(&nobody(), false), Step::Nothing);
    }
    assert_eq!(run.rules.tracked(), None);
}

#[test]
fn the_line_names_the_app() {
    assert_eq!(ended_line(&zoom()), "Zoom call ended");
    assert_eq!(ended_line(&whatsapp()), "WhatsApp call ended");
    assert_eq!(
        ended_line(&MicUser::new(
            "com.apple.FaceTime",
            "FaceTime",
            MicKind::CallApp
        )),
        "FaceTime call ended"
    );
    assert_eq!(
        ended_line(&MicUser::new(
            "callservicesd",
            "Phone call",
            MicKind::CallApp
        )),
        "Phone call ended"
    );
    assert_eq!(
        ended_line(&MicUser::new(
            "com.tinyspeck.slackmacgap",
            "Slack",
            MicKind::CallApp
        )),
        "Slack call ended"
    );
    assert_eq!(ended_line(&chrome()), "Call ended in Google Chrome");
}

#[test]
fn diff_apps_lists_who_started_and_who_stopped() {
    let before = [zoom(), chrome()];
    let after = [chrome(), whatsapp()];
    let (started, stopped) = diff_apps(&before, &after);
    assert_eq!(started, [&whatsapp()]);
    assert_eq!(stopped, [&zoom()]);
    let (started, stopped) = diff_apps(&after, &after);
    assert!(started.is_empty() && stopped.is_empty());
}

#[test]
fn ids_never_repeat_across_countdowns() {
    let mut run = Run::new();
    let mut ids = Vec::new();
    for _ in 0..5 {
        run.reads(&on(&[zoom()]), 2);
        let (id, _) = run.until_shown();
        ids.push(id);
        run.rules.answer(id, Answer::KeepRecording);
    }
    let mut unique = ids.clone();
    unique.dedup();
    assert_eq!(ids, unique);
    assert!(ids.iter().all(|id| *id != 0));
}
