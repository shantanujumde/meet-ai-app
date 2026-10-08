//! A call starting in an app we can name (TUR-143; TUR-141 decisions 2, 3,
//! 5, 14 and 15).
//!
//! The app reads which apps are using a mic every [`MIC_POLL_INTERVAL`]
//! (`audio::mic_users`, TUR-142) and hands each reading here as
//! [`MicUser`]s. `detect` does not depend on `audio`, so the app copies each
//! app into this crate's own type. [`CallStart`] is pure state: readings and
//! a time in, at most one [`CallStarted`] out, so every rule is a unit test
//! with fake readings and a fake clock.
//!
//! The rules:
//!
//! 1. **A call app or a browser, on the mic for [`CALL_START_HOLD`].** Only
//!    [`MicKind::CallApp`] and [`MicKind::Browser`] count. A reading without
//!    the app restarts its hold. An app we do not know ([`MicKind::Other`])
//!    never prompts here: it still goes through TUR-31's mic-and-speakers
//!    rule ([`audio_activity_allowed`]).
//! 2. **Never a call.** Dictation, voice assistants and audio-routing tools
//!    ([`MicKind::Ignored`], and the built-in [`NEVER_A_CALL`] list, which
//!    also holds meet-ai itself) and every app on the user's "Never detect"
//!    list are skipped, whatever they do.
//! 3. **Once per call.** A prompt, or a recording running while the app is on
//!    the mic, makes that stretch of mic use handled until the app lets go
//!    of the mic. A recording is "already recording": no prompt, as
//!    `crate::detector` treats a meeting app seen while recording.
//! 4. **Not now means 10 minutes.** After a prompt that did not lead to a
//!    recording (Not now, or no answer) the same app does not ask again for
//!    [`NOT_NOW_COOLDOWN`], even if it lets go of the mic and picks it up
//!    again. [`CallStart::dismissed`] restarts that time from the click. A
//!    recording clears the cooldowns no one dismissed, so a Record that was
//!    then stopped does not keep the next call quiet.
//! 5. **One call, one prompt.** When several apps are ready at once (a Zoom
//!    call with a browser tab also holding the mic), one prompt names one of
//!    them, a call app before a browser, and the rest count as handled.
//!
//! Nothing here records (L15): what comes out is a reason to ask.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often the app reads which apps use the mic.
pub const MIC_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long a call app or browser must hold the mic before we ask.
/// Long enough to skip a voice message or a mic test; short enough to ask
/// before anything worth keeping is said.
pub const CALL_START_HOLD: Duration = Duration::from_secs(15);

/// How long an app stays quiet after a prompt about it was not answered with
/// Record.
pub const NOT_NOW_COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// Apps that use the mic without being on a call, whatever the app table in
/// `audio` says (TUR-141 decision 14): meet-ai itself, Krisp, Rogue Amoeba's
/// apps, BlackHole, Superwhisper, Wispr Flow, MacWhisper, Raycast and Siri
/// (`corespeechd`). Each entry is a bundle id (which also covers its
/// sub-ids, `com.rogueamoeba.Loopback` under `com.rogueamoeba`) or an app
/// name, both compared without case. BlackHole is an audio driver, not a
/// process, so it should never show up; it is here in case a helper does.
pub const NEVER_A_CALL: &[&str] = &[
    "pro.saleschat.meetai",
    "meet-ai",
    "ai.krisp.krispMac",
    "Krisp",
    "com.rogueamoeba",
    "Loopback",
    "Audio Hijack",
    "SoundSource",
    "BlackHole",
    "Superwhisper",
    "Wispr Flow",
    "MacWhisper",
    "com.raycast.macos",
    "Raycast",
    "com.apple.CoreSpeech",
    "corespeechd",
    "Siri",
];

/// What sort of app is using the mic (`audio::mic_users::AppKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicKind {
    /// An app made for calls: Zoom, Teams, WhatsApp, FaceTime.
    CallApp,
    /// A web browser: the call is in one of its tabs.
    Browser,
    /// Anything else: TUR-31's rule decides.
    Other,
    /// Never a call: dictation, assistants, audio routing.
    Ignored,
}

/// One app using a mic, as `audio::mic_users` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicUser {
    /// A stable id: the bundle id on macOS (`com.google.Chrome`), the
    /// lower-cased program name elsewhere (`chrome.exe`).
    pub id: String,
    /// The app's name for the UI: "WhatsApp", "Google Chrome".
    pub name: String,
    pub kind: MicKind,
}

impl MicUser {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: MicKind) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
        }
    }
}

/// One reading of who uses the mic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicReading {
    /// The OS listed the apps (empty: nobody but meet-ai).
    Supported(Vec<MicUser>),
    /// This OS cannot say which apps use a mic (macOS older than 14): the
    /// TUR-27 and TUR-31 rules run as before.
    NotSupported,
}

/// A call just started in `name`: ask whether to record it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallStarted {
    pub id: String,
    pub name: String,
    /// The call is in a browser tab: "Call detected in Google Chrome".
    pub browser: bool,
}

/// `user` is named by `entry`: its id, a sub-id of it, or its name, without
/// case.
fn named_by(user: &MicUser, entry: &str) -> bool {
    let entry = entry.trim();
    if entry.is_empty() {
        return false;
    }
    let id = user.id.to_lowercase();
    let wanted = entry.to_lowercase();
    id == wanted
        || id
            .strip_prefix(&wanted)
            .is_some_and(|rest| rest.starts_with('.'))
        || user.name.trim().eq_ignore_ascii_case(entry)
}

/// `user` never counts as a call: [`MicKind::Ignored`], on [`NEVER_A_CALL`],
/// or on the user's "Never detect" list (names or ids).
pub fn never_a_call(user: &MicUser, never: &[String]) -> bool {
    user.kind == MicKind::Ignored
        || NEVER_A_CALL.iter().any(|entry| named_by(user, entry))
        || never.iter().any(|entry| named_by(user, entry))
}

/// `user` is one this rule asks about: a call app or a browser that is not
/// [`never_a_call`].
pub fn may_start_a_call(user: &MicUser, never: &[String]) -> bool {
    matches!(user.kind, MicKind::CallApp | MicKind::Browser) && !never_a_call(user, never)
}

/// May TUR-31's "mic and speakers both in use" prompt still ask, given the
/// latest reading? Always where the OS cannot list the apps. Where it can,
/// only when an app we do not know is on the mic and no call app or browser
/// is (that one asks through [`CallStart`] instead): dictation or Krisp with
/// music playing is not a call.
pub fn audio_activity_allowed(reading: &MicReading, never: &[String]) -> bool {
    match reading {
        MicReading::NotSupported => true,
        MicReading::Supported(users) => {
            !users.iter().any(|user| may_start_a_call(user, never))
                && users
                    .iter()
                    .any(|user| user.kind == MicKind::Other && !never_a_call(user, never))
        }
    }
}

/// One app's current stretch on the mic.
#[derive(Debug)]
struct OnMic {
    since: Instant,
    /// Prompted for, or seen while recording: quiet until it lets go.
    handled: bool,
}

/// An app that asked and was not recorded.
#[derive(Debug)]
struct Cooldown {
    until: Instant,
    /// Set by an explicit Not now, which a later recording does not clear.
    dismissed: bool,
}

/// What the call-start rule remembers between readings.
#[derive(Debug, Default)]
pub struct CallStart {
    /// By app id: the apps on the mic in the last reading.
    on_mic: HashMap<String, OnMic>,
    /// By lower-cased app name, the name the prompt and Not now carry.
    cooldowns: HashMap<String, Cooldown>,
}

impl CallStart {
    pub fn new() -> Self {
        Self::default()
    }

    /// One reading at `now`: the apps on the mic, the user's "Never detect"
    /// list, and whether meet-ai is recording. Returns the call to ask
    /// about, if one just started.
    pub fn observe(
        &mut self,
        users: &[MicUser],
        never: &[String],
        recording: bool,
        now: Instant,
    ) -> Option<CallStarted> {
        self.cooldowns.retain(|_, cooldown| now < cooldown.until);
        if recording {
            self.cooldowns.retain(|_, cooldown| cooldown.dismissed);
        }
        let candidates: Vec<&MicUser> = users
            .iter()
            .filter(|user| may_start_a_call(user, never))
            .collect();
        // An app that let go of the mic re-arms: its next stretch is a new call.
        self.on_mic
            .retain(|id, _| candidates.iter().any(|user| &user.id == id));

        let mut ready: Vec<&MicUser> = Vec::new();
        for user in candidates {
            let cooling = self.cooldowns.contains_key(&key(&user.name));
            let on_mic = self.on_mic.entry(user.id.clone()).or_insert(OnMic {
                since: now,
                handled: false,
            });
            if recording || cooling {
                on_mic.handled = true;
            }
            if on_mic.handled {
                continue;
            }
            if now.saturating_duration_since(on_mic.since) >= CALL_START_HOLD {
                ready.push(user);
            }
        }
        // A call app names the call better than a browser that also holds the mic.
        ready.sort_by_key(|user| (user.kind != MicKind::CallApp, user.name.to_lowercase()));
        let first = ready.first()?;
        for user in &ready {
            if let Some(on_mic) = self.on_mic.get_mut(&user.id) {
                on_mic.handled = true;
            }
        }
        self.cooldowns.insert(
            key(&first.name),
            Cooldown {
                until: now + NOT_NOW_COOLDOWN,
                dismissed: false,
            },
        );
        Some(CallStarted {
            id: first.id.clone(),
            name: first.name.clone(),
            browser: first.kind == MicKind::Browser,
        })
    }

    /// The user pressed Not now on the prompt about `app` (its name) at
    /// `now`: quiet for [`NOT_NOW_COOLDOWN`] from here.
    pub fn dismissed(&mut self, app: &str, now: Instant) {
        self.cooldowns.insert(
            key(app),
            Cooldown {
                until: now + NOT_NOW_COOLDOWN,
                dismissed: true,
            },
        );
    }
}

fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLL: Duration = MIC_POLL_INTERVAL;

    fn whatsapp() -> MicUser {
        MicUser::new("net.whatsapp.WhatsApp", "WhatsApp", MicKind::CallApp)
    }

    fn chrome() -> MicUser {
        MicUser::new("com.google.Chrome", "Google Chrome", MicKind::Browser)
    }

    fn started(user: &MicUser) -> Option<CallStarted> {
        Some(CallStarted {
            id: user.id.clone(),
            name: user.name.clone(),
            browser: user.kind == MicKind::Browser,
        })
    }

    /// Feed the same reading every poll from `from` for `length`; every
    /// prompt that came out, with the time it came.
    fn run(
        rule: &mut CallStart,
        users: &[MicUser],
        never: &[String],
        recording: bool,
        from: Instant,
        length: Duration,
    ) -> Vec<(Duration, CallStarted)> {
        let mut out = Vec::new();
        let mut at = Duration::ZERO;
        while at <= length {
            if let Some(call) = rule.observe(users, never, recording, from + at) {
                out.push((at, call));
            }
            at += POLL;
        }
        out
    }

    #[test]
    fn a_call_app_on_the_mic_for_15_s_asks_once_naming_it() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let prompts = run(
            &mut rule,
            &[whatsapp()],
            &[],
            false,
            start,
            Duration::from_secs(3600),
        );
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        assert_eq!(
            prompts[0].0,
            Duration::from_secs(16),
            "the first poll past 15 s"
        );
        assert_eq!(Some(prompts[0].1.clone()), started(&whatsapp()));
    }

    #[test]
    fn the_hold_is_15_s_exactly() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(rule.observe(&users, &[], false, start), None);
        assert_eq!(
            rule.observe(&users, &[], false, start + Duration::from_secs(14)),
            None
        );
        assert_eq!(
            rule.observe(&users, &[], false, start + CALL_START_HOLD),
            started(&whatsapp())
        );
    }

    #[test]
    fn a_reading_without_the_app_restarts_the_hold() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(rule.observe(&users, &[], false, start), None);
        assert_eq!(
            rule.observe(&[], &[], false, start + Duration::from_secs(10)),
            None
        );
        assert_eq!(
            rule.observe(&users, &[], false, start + Duration::from_secs(12)),
            None
        );
        assert_eq!(
            rule.observe(&users, &[], false, start + Duration::from_secs(26)),
            None,
            "only 14 s since it came back"
        );
        assert_eq!(
            rule.observe(&users, &[], false, start + Duration::from_secs(27)),
            started(&whatsapp())
        );
    }

    #[test]
    fn a_browser_asks_as_a_browser() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let prompts = run(
            &mut rule,
            &[chrome()],
            &[],
            false,
            start,
            Duration::from_secs(30),
        );
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].1.browser);
        assert_eq!(prompts[0].1.name, "Google Chrome");
    }

    #[test]
    fn an_app_we_do_not_know_never_asks_here() {
        let mut rule = CallStart::new();
        let recorder = MicUser::new("com.example.recorder", "Recorder", MicKind::Other);
        let prompts = run(
            &mut rule,
            &[recorder],
            &[],
            false,
            Instant::now(),
            Duration::from_secs(3600),
        );
        assert!(prompts.is_empty());
    }

    #[test]
    fn dictation_apps_and_krisp_never_ask() {
        let users = [
            // As the app table names them.
            MicUser::new("com.raycast.macos", "Raycast", MicKind::Ignored),
            MicUser::new("com.apple.CoreSpeech", "Siri", MicKind::Ignored),
            // Even when a table miss calls them a call app or a browser.
            MicUser::new("ai.krisp.krispMac", "krisp", MicKind::CallApp),
            MicUser::new("com.rogueamoeba.Loopback", "Loopback", MicKind::CallApp),
            MicUser::new("com.example.superwhisper", "Superwhisper", MicKind::Browser),
            MicUser::new("wispr flow.exe", "Wispr Flow", MicKind::CallApp),
            MicUser::new("com.example.macwhisper", "MacWhisper", MicKind::CallApp),
            MicUser::new("x", "corespeechd", MicKind::CallApp),
            MicUser::new("x.blackhole", "BlackHole", MicKind::CallApp),
            MicUser::new("pro.saleschat.meetai", "meet-ai", MicKind::CallApp),
        ];
        for user in &users {
            assert!(never_a_call(user, &[]), "{user:?}");
        }
        let mut rule = CallStart::new();
        let prompts = run(
            &mut rule,
            &users,
            &[],
            false,
            Instant::now(),
            Duration::from_secs(3600),
        );
        assert!(prompts.is_empty(), "{prompts:?}");
    }

    #[test]
    fn the_never_detect_list_matches_names_and_ids_without_case() {
        let mut rule = CallStart::new();
        for never in [
            vec!["WhatsApp".to_string()],
            vec!["whatsapp".to_string()],
            vec!["net.whatsapp.WhatsApp".to_string()],
            vec!["  WhatsApp ".to_string()],
        ] {
            let prompts = run(
                &mut rule,
                &[whatsapp()],
                &never,
                false,
                Instant::now(),
                Duration::from_secs(60),
            );
            assert!(prompts.is_empty(), "{never:?}");
        }
        // Not a prefix of the name, and not an empty entry.
        assert!(!never_a_call(&whatsapp(), &["Whats".to_string()]));
        assert!(!never_a_call(&whatsapp(), &[String::new()]));
        assert!(!never_a_call(
            &whatsapp(),
            &["net.whatsapp.WhatsAppX".to_string()]
        ));
    }

    #[test]
    fn a_never_detect_app_does_not_hide_another_one() {
        let mut rule = CallStart::new();
        let prompts = run(
            &mut rule,
            &[chrome(), whatsapp()],
            &["Google Chrome".to_string()],
            false,
            Instant::now(),
            Duration::from_secs(60),
        );
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].1.name, "WhatsApp");
    }

    #[test]
    fn already_recording_never_asks_and_the_call_counts_as_handled() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                true,
                start,
                Duration::from_secs(600)
            )
            .is_empty()
        );
        // The recording stops; the same call goes on: still handled.
        let after = start + Duration::from_secs(602);
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                after,
                Duration::from_secs(600)
            )
            .is_empty()
        );
        // The call ends, and a new one asks as usual.
        let later = after + Duration::from_secs(602);
        assert_eq!(rule.observe(&[], &[], false, later), None);
        let prompts = run(
            &mut rule,
            &users,
            &[],
            false,
            later + POLL,
            Duration::from_secs(30),
        );
        assert_eq!(prompts.len(), 1);
    }

    #[test]
    fn a_recording_mid_hold_counts_the_call_as_handled() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(rule.observe(&users, &[], false, start), None);
        assert_eq!(
            rule.observe(&users, &[], true, start + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            rule.observe(&users, &[], false, start + Duration::from_secs(30)),
            None
        );
    }

    #[test]
    fn not_now_keeps_the_app_quiet_for_10_min_even_across_calls() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        let first = run(
            &mut rule,
            &users,
            &[],
            false,
            start,
            Duration::from_secs(20),
        );
        assert_eq!(first.len(), 1);
        let asked = start + first[0].0;
        rule.dismissed("WhatsApp", asked + Duration::from_secs(5));
        // Hang up, call again a minute later: quiet.
        let again = asked + Duration::from_secs(60);
        assert_eq!(rule.observe(&[], &[], false, again), None);
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                again + POLL,
                Duration::from_secs(120)
            )
            .is_empty()
        );
        // Hang up; past 10 min from the click, a new call asks.
        let past = asked + Duration::from_secs(5) + NOT_NOW_COOLDOWN;
        assert_eq!(rule.observe(&[], &[], false, past), None);
        let prompts = run(
            &mut rule,
            &users,
            &[],
            false,
            past + POLL,
            Duration::from_secs(20),
        );
        assert_eq!(prompts.len(), 1);
    }

    #[test]
    fn a_new_call_during_the_cooldown_stays_quiet_after_it_ends() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                start,
                Duration::from_secs(16)
            )
            .len(),
            1
        );
        rule.dismissed("whatsapp", start + Duration::from_secs(17));
        assert_eq!(
            rule.observe(&[], &[], false, start + Duration::from_secs(60)),
            None
        );
        // A call that starts at minute 5 and runs to minute 30 never asks.
        let five = start + Duration::from_secs(300);
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                five,
                Duration::from_secs(1500)
            )
            .is_empty()
        );
    }

    #[test]
    fn an_unanswered_prompt_counts_as_not_now() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                start,
                Duration::from_secs(16)
            )
            .len(),
            1
        );
        assert_eq!(
            rule.observe(&[], &[], false, start + Duration::from_secs(40)),
            None
        );
        let again = start + Duration::from_secs(120);
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                again,
                Duration::from_secs(60)
            )
            .is_empty()
        );
    }

    #[test]
    fn record_then_stop_does_not_keep_the_next_call_quiet() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        let users = [whatsapp()];
        assert_eq!(
            run(
                &mut rule,
                &users,
                &[],
                false,
                start,
                Duration::from_secs(16)
            )
            .len(),
            1
        );
        // Record was pressed: the recording runs for the rest of the call.
        let recorded = start + Duration::from_secs(18);
        assert!(
            run(
                &mut rule,
                &users,
                &[],
                true,
                recorded,
                Duration::from_secs(120)
            )
            .is_empty()
        );
        let hung_up = recorded + Duration::from_secs(122);
        assert_eq!(rule.observe(&[], &[], false, hung_up), None);
        // The next call, two minutes later, asks.
        let next = hung_up + Duration::from_secs(120);
        assert_eq!(
            run(&mut rule, &users, &[], false, next, Duration::from_secs(20)).len(),
            1
        );
    }

    #[test]
    fn a_recording_does_not_undo_a_not_now() {
        let mut rule = CallStart::new();
        let start = Instant::now();
        rule.dismissed("WhatsApp", start);
        assert_eq!(rule.observe(&[], &[], true, start + POLL), None);
        assert!(
            run(
                &mut rule,
                &[whatsapp()],
                &[],
                false,
                start + Duration::from_secs(60),
                Duration::from_secs(60)
            )
            .is_empty()
        );
    }

    #[test]
    fn not_now_for_one_app_leaves_the_others() {
        let mut rule = CallStart::new();
        rule.dismissed("WhatsApp", Instant::now());
        let prompts = run(
            &mut rule,
            &[chrome()],
            &[],
            false,
            Instant::now(),
            Duration::from_secs(20),
        );
        assert_eq!(prompts.len(), 1);
    }

    #[test]
    fn two_apps_at_once_ask_once_naming_the_call_app() {
        let mut rule = CallStart::new();
        let zoom = MicUser::new("us.zoom.xos", "Zoom", MicKind::CallApp);
        let prompts = run(
            &mut rule,
            &[chrome(), zoom.clone()],
            &[],
            false,
            Instant::now(),
            Duration::from_secs(600),
        );
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        assert_eq!(Some(prompts[0].1.clone()), started(&zoom));
    }

    #[test]
    fn teams_open_all_day_with_no_call_never_asks() {
        // Teams is open but not on the mic, so it is not in any reading.
        let mut rule = CallStart::new();
        let prompts = run(
            &mut rule,
            &[],
            &[],
            false,
            Instant::now(),
            Duration::from_secs(8 * 3600),
        );
        assert!(prompts.is_empty());
    }

    #[test]
    fn audio_activity_asks_only_for_an_app_we_do_not_know() {
        let recorder = MicUser::new("com.example.recorder", "Recorder", MicKind::Other);
        let krisp = MicUser::new("ai.krisp.krispMac", "Krisp", MicKind::Ignored);
        assert!(audio_activity_allowed(&MicReading::NotSupported, &[]));
        assert!(audio_activity_allowed(
            &MicReading::Supported(vec![recorder.clone()]),
            &[]
        ));
        assert!(audio_activity_allowed(
            &MicReading::Supported(vec![recorder.clone(), krisp.clone()]),
            &[]
        ));
        // Dictation or Krisp with music playing is not a call.
        assert!(!audio_activity_allowed(
            &MicReading::Supported(vec![krisp]),
            &[]
        ));
        assert!(!audio_activity_allowed(
            &MicReading::Supported(Vec::new()),
            &[]
        ));
        // A call app or browser asks through the call-start rule instead.
        assert!(!audio_activity_allowed(
            &MicReading::Supported(vec![recorder.clone(), whatsapp()]),
            &[]
        ));
        // An app on the "Never detect" list is no reason to ask either.
        assert!(!audio_activity_allowed(
            &MicReading::Supported(vec![recorder.clone()]),
            &["Recorder".to_string()]
        ));
        // But a never-detected call app no longer blocks it.
        assert!(audio_activity_allowed(
            &MicReading::Supported(vec![recorder, whatsapp()]),
            &["WhatsApp".to_string()]
        ));
    }
}
