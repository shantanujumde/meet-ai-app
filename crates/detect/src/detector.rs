//! When to ask: the rules that keep detection from nagging.
//!
//! Pure state, no clock and no OS — the loop in [`crate::poll`] feeds it a
//! process list and a time, so every rule is a plain unit test.
//!
//! The rules, settled in TUR-27:
//!
//! 1. **No prompt while recording.** An app seen while a recording runs counts
//!    as handled: the user is already recording, most likely that very call,
//!    and asking the moment they press Stop would be the worst-timed prompt.
//! 2. **Once per app session.** A session is the set of process ids running
//!    under a meeting app's name. Its first sighting prompts once; it re-arms
//!    only when every one of those pids has exited — quitting and reopening
//!    Zoom asks again, dismissing does not.
//! 3. **Slack and Discord need a call signal too** (`needs_call_signal` in
//!    `processes.json`). They are open all day, so they only prompt when a
//!    calendar event or audio activity was seen in the last
//!    [`CALL_SIGNAL_WINDOW`]. Until a signal arrives they stay un-handled, so a
//!    signal later in the same session can still prompt once. Until calendar
//!    (TUR-26) and audio activity feed [`Detector::call_signal`], they never
//!    prompt alone.
//! 4. **One prompt per call, from any signal.** Audio activity (TUR-31,
//!    [`Detector::audio_activity`]) counts as a call signal first, so a Slack
//!    call is named as Slack. It prompts on its own only when no meeting app
//!    prompts for it and nothing prompted in the last [`CALL_SIGNAL_WINDOW`]
//!    — Zoom opening and then its call starting asks once, not twice.
//! 5. **One app, one session, whatever its processes are called.** Apps are
//!    grouped by their `processes.json` label, so new Teams on Windows
//!    (`ms-teams.exe` plus the `ms-teams_modulehost.exe` that holds the call)
//!    is one Teams session and asks once (TUR-60).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::Signal;
use crate::processes;

/// How recent a calendar or audio-activity signal must be for Slack or
/// Discord being open to count as a call.
pub const CALL_SIGNAL_WINDOW: Duration = Duration::from_secs(5 * 60);

/// One running process, as the OS lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub name: String,
}

impl RunningProcess {
    pub fn new(pid: u32, name: impl Into<String>) -> Self {
        Self {
            pid,
            name: name.into(),
        }
    }
}

/// What detection remembers between polls.
#[derive(Debug, Default)]
pub struct Detector {
    /// Per meeting app (its `processes.json` label), the pids of the session
    /// already prompted for or seen while recording.
    handled: HashMap<String, HashSet<u32>>,
    /// When the last calendar or audio-activity signal arrived.
    last_call_signal: Option<Instant>,
    /// When the last prompt of any kind was asked for.
    last_prompt: Option<Instant>,
}

impl Detector {
    pub fn new() -> Self {
        Self::default()
    }

    /// A calendar event or audio activity was seen at `at`.
    pub fn call_signal(&mut self, at: Instant) {
        self.last_call_signal = Some(self.last_call_signal.map_or(at, |last| last.max(at)));
    }

    fn call_signal_recent(&self, now: Instant) -> bool {
        self.last_call_signal
            .is_some_and(|at| now.saturating_duration_since(at) <= CALL_SIGNAL_WINDOW)
    }

    /// One poll: the processes running now, whether a recording is running,
    /// and the time. Returns the signals to prompt for — at most one per
    /// meeting app.
    pub fn observe(
        &mut self,
        running: &[RunningProcess],
        recording: bool,
        now: Instant,
    ) -> Vec<Signal> {
        // The meeting apps running now, in processes.json order, with their pids.
        let mut apps: Vec<(&'static processes::MeetingProcess, HashSet<u32>)> = Vec::new();
        for process in running {
            let Some(known) = processes::find(&process.name) else {
                continue;
            };
            match apps.iter_mut().find(|(app, _)| app.label == known.label) {
                Some((_, pids)) => {
                    pids.insert(process.pid);
                }
                None => apps.push((known, HashSet::from([process.pid]))),
            }
        }

        // Re-arm every session whose pids have all exited.
        self.handled.retain(|name, handled| {
            let alive = apps
                .iter()
                .find(|(app, _)| &app.label == name)
                .map(|(_, pids)| pids);
            handled.retain(|pid| alive.is_some_and(|pids| pids.contains(pid)));
            !handled.is_empty()
        });

        let mut signals = Vec::new();
        for (app, pids) in apps {
            if let Some(handled) = self.handled.get_mut(&app.label) {
                // The same session, perhaps with a new helper pid: keep them
                // all, so the session lasts until the last one exits.
                handled.extend(pids);
                continue;
            }
            if recording {
                self.handled.insert(app.label.clone(), pids);
                continue;
            }
            if app.needs_call_signal && !self.call_signal_recent(now) {
                continue;
            }
            self.handled.insert(app.label.clone(), pids);
            signals.push(Signal::Process {
                process: app.name.clone(),
            });
        }
        if !signals.is_empty() {
            self.last_prompt = Some(now);
        }
        signals
    }

    /// Audio activity was seen at `now` (see [`crate::activity`]): a poll
    /// like [`Self::observe`] that also counts it as a call signal. Returns
    /// the meeting apps that now prompt; failing those,
    /// [`Signal::AudioActivity`] — unless recording, or something prompted in
    /// the last [`CALL_SIGNAL_WINDOW`] (most likely for this very call).
    pub fn audio_activity(
        &mut self,
        running: &[RunningProcess],
        recording: bool,
        now: Instant,
    ) -> Vec<Signal> {
        let recent_prompt = self
            .last_prompt
            .is_some_and(|at| now.saturating_duration_since(at) <= CALL_SIGNAL_WINDOW);
        self.call_signal(now);
        let signals = self.observe(running, recording, now);
        if !signals.is_empty() || recording || recent_prompt {
            return signals;
        }
        self.last_prompt = Some(now);
        vec![Signal::AudioActivity]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processes::name_of;

    fn zoom(pid: u32) -> RunningProcess {
        RunningProcess::new(pid, name_of("Zoom"))
    }

    fn process(name: &str) -> Signal {
        Signal::Process {
            process: name.to_string(),
        }
    }

    #[test]
    fn a_meeting_app_appearing_prompts_once_naming_it() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let running = [RunningProcess::new(1, "Finder"), zoom(42)];
        assert_eq!(
            detector.observe(&running, false, now),
            [process(name_of("Zoom"))]
        );
        assert!(detector.observe(&running, false, now).is_empty());
        assert!(
            detector
                .observe(&running, false, now + Duration::from_secs(3600))
                .is_empty()
        );
    }

    #[test]
    fn nothing_known_running_prompts_nothing() {
        let mut detector = Detector::new();
        let running = [
            RunningProcess::new(1, "Finder"),
            RunningProcess::new(2, "zoom helper"),
            RunningProcess::new(3, "Slack Helper"),
        ];
        assert!(detector.observe(&running, false, Instant::now()).is_empty());
    }

    #[test]
    fn matching_ignores_case() {
        let mut detector = Detector::new();
        let running = [RunningProcess::new(7, name_of("Zoom").to_uppercase())];
        assert_eq!(
            detector.observe(&running, false, Instant::now()),
            [process(name_of("Zoom"))]
        );
    }

    #[test]
    fn a_dismissed_app_asks_again_only_after_it_quits() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(detector.observe(&[zoom(42)], false, now).len(), 1);
        // Dismissed: still open, never again.
        assert!(detector.observe(&[zoom(42)], false, now).is_empty());
        // Quit…
        assert!(detector.observe(&[], false, now).is_empty());
        // …and reopened: a new session.
        assert_eq!(
            detector.observe(&[zoom(43)], false, now),
            [process(name_of("Zoom"))]
        );
    }

    #[test]
    fn a_restart_between_two_polls_is_a_new_session() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(detector.observe(&[zoom(42)], false, now).len(), 1);
        assert_eq!(detector.observe(&[zoom(99)], false, now).len(), 1);
    }

    #[test]
    fn a_second_pid_under_the_same_name_is_the_same_session() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(detector.observe(&[zoom(42)], false, now).len(), 1);
        assert!(
            detector
                .observe(&[zoom(42), zoom(43)], false, now)
                .is_empty()
        );
        // The first exits; the second keeps the session alive.
        assert!(detector.observe(&[zoom(43)], false, now).is_empty());
    }

    #[test]
    fn two_apps_are_two_sessions() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let running = [zoom(1), RunningProcess::new(2, name_of("Webex"))];
        assert_eq!(
            detector.observe(&running, false, now),
            [process(name_of("Zoom")), process(name_of("Webex"))]
        );
        // Quitting Webex re-arms only Webex.
        assert!(detector.observe(&[zoom(1)], false, now).is_empty());
        assert_eq!(
            detector.observe(
                &[zoom(1), RunningProcess::new(3, name_of("Webex"))],
                false,
                now
            ),
            [process(name_of("Webex"))]
        );
    }

    #[test]
    fn no_prompt_while_recording() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert!(detector.observe(&[zoom(42)], true, now).is_empty());
    }

    #[test]
    fn an_app_seen_while_recording_does_not_prompt_when_the_recording_stops() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert!(detector.observe(&[zoom(42)], true, now).is_empty());
        assert!(detector.observe(&[zoom(42)], false, now).is_empty());
        // The next session, after it quits, asks as usual.
        assert!(detector.observe(&[], false, now).is_empty());
        assert_eq!(detector.observe(&[zoom(43)], false, now).len(), 1);
    }

    #[test]
    fn slack_alone_never_prompts() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let running = [
            RunningProcess::new(5, name_of("Slack")),
            RunningProcess::new(6, name_of("Discord")),
        ];
        for minute in 0..60 {
            let at = now + Duration::from_secs(minute * 60);
            assert!(detector.observe(&running, false, at).is_empty());
        }
    }

    #[test]
    fn slack_prompts_with_a_recent_call_signal_then_not_again() {
        let mut detector = Detector::new();
        let start = Instant::now();
        let slack = [RunningProcess::new(5, name_of("Slack"))];
        assert!(detector.observe(&slack, false, start).is_empty());

        let signal_at = start + Duration::from_secs(600);
        detector.call_signal(signal_at);
        let at = signal_at + Duration::from_secs(5);
        assert_eq!(
            detector.observe(&slack, false, at),
            [process(name_of("Slack"))]
        );
        assert!(detector.observe(&slack, false, at).is_empty());

        // A later signal in the same Slack session still does not nag.
        detector.call_signal(at + Duration::from_secs(3600));
        assert!(
            detector
                .observe(&slack, false, at + Duration::from_secs(3601))
                .is_empty()
        );
    }

    #[test]
    fn a_call_signal_older_than_the_window_does_not_count() {
        let mut detector = Detector::new();
        let signal_at = Instant::now();
        detector.call_signal(signal_at);
        let slack = [RunningProcess::new(5, name_of("Slack"))];
        let late = signal_at + CALL_SIGNAL_WINDOW + Duration::from_secs(1);
        assert!(detector.observe(&slack, false, late).is_empty());
        let edge = [RunningProcess::new(6, name_of("Discord"))];
        assert_eq!(
            detector.observe(&edge, false, signal_at + CALL_SIGNAL_WINDOW),
            [process(name_of("Discord"))]
        );
    }

    #[test]
    fn slack_with_a_call_signal_still_waits_while_recording() {
        let mut detector = Detector::new();
        let now = Instant::now();
        detector.call_signal(now);
        let slack = [RunningProcess::new(5, name_of("Slack"))];
        assert!(detector.observe(&slack, true, now).is_empty());
        assert!(detector.observe(&slack, false, now).is_empty());
    }

    #[test]
    fn audio_activity_alone_prompts_as_audio_activity() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let running = [RunningProcess::new(1, "Google Chrome")];
        assert_eq!(
            detector.audio_activity(&running, false, now),
            [Signal::AudioActivity]
        );
    }

    #[test]
    fn audio_activity_names_slack_instead() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let slack = [RunningProcess::new(5, name_of("Slack"))];
        assert!(detector.observe(&slack, false, now).is_empty());
        assert_eq!(
            detector.audio_activity(&slack, false, now + Duration::from_secs(5)),
            [process(name_of("Slack"))]
        );
    }

    #[test]
    fn audio_activity_soon_after_an_app_prompt_stays_quiet() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(detector.observe(&[zoom(42)], false, now).len(), 1);
        // Zoom's call starts a minute later: already asked.
        assert!(
            detector
                .audio_activity(&[zoom(42)], false, now + Duration::from_secs(60))
                .is_empty()
        );
        // Hours later, with Zoom still open, a new call asks.
        assert_eq!(
            detector.audio_activity(&[zoom(42)], false, now + Duration::from_secs(3 * 3600)),
            [Signal::AudioActivity]
        );
    }

    #[test]
    fn zoom_first_seen_with_audio_activity_asks_once_naming_zoom() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(
            detector.audio_activity(&[zoom(42)], false, now),
            [process(name_of("Zoom"))]
        );
        assert!(
            detector
                .observe(&[zoom(42)], false, now + Duration::from_secs(5))
                .is_empty()
        );
    }

    #[test]
    fn audio_activity_while_recording_stays_quiet() {
        let mut detector = Detector::new();
        assert!(
            detector
                .audio_activity(&[], true, Instant::now())
                .is_empty()
        );
    }

    #[test]
    fn two_audio_activities_in_a_row_ask_once() {
        let mut detector = Detector::new();
        let now = Instant::now();
        assert_eq!(detector.audio_activity(&[], false, now).len(), 1);
        assert!(
            detector
                .audio_activity(&[], false, now + Duration::from_secs(90))
                .is_empty()
        );
    }

    /// Every process name this OS has for Teams (`ms-teams.exe` and
    /// `ms-teams_modulehost.exe` on Windows, say), all running at once.
    fn every_teams_process() -> Vec<RunningProcess> {
        crate::processes::meeting_processes()
            .iter()
            .filter(|known| known.label == "Microsoft Teams")
            .zip(100..)
            .map(|(known, pid)| RunningProcess::new(pid, known.name.as_str()))
            .collect()
    }

    #[test]
    fn every_teams_process_together_asks_once() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let teams = every_teams_process();
        assert!(!teams.is_empty());
        assert_eq!(
            detector.observe(&teams, false, now),
            [process(&teams[0].name)]
        );
        // The calling helper starting later is the same session.
        assert!(detector.observe(&teams, false, now).is_empty());
        let mut reversed = teams.clone();
        reversed.reverse();
        assert!(detector.observe(&reversed, false, now).is_empty());
    }

    #[test]
    fn the_teams_session_lasts_until_its_last_process_exits() {
        let mut detector = Detector::new();
        let now = Instant::now();
        let teams = every_teams_process();
        assert_eq!(detector.observe(&teams, false, now).len(), 1);
        let last = &teams[teams.len() - 1..];
        assert!(detector.observe(last, false, now).is_empty());
        assert!(detector.observe(&[], false, now).is_empty());
        assert_eq!(detector.observe(last, false, now).len(), 1);
    }
}
