//! The audio retention job (SPEC L16, TUR-45): deletes meeting WAVs once
//! they are older than `audio.retention_days`.
//!
//! It runs about [`FIRST_RUN_DELAY`] after launch, then every [`RUN_EVERY`].
//! With `retention_days: 0` it also runs as soon as a meeting's transcript is
//! final ([`Transcribing::settled`]) and again when its notes run ends
//! ([`after_notes_run`]), since a busy meeting is skipped. Deciding what goes
//! and deleting it is `store::retention`'s job; this module knows what the app
//! is busy with and when to run.
//!
//! Busy, and so never touched, means: being recorded, being transcribed
//! (from Stop until `transcript.md` is final), or a notes or Sync run going
//! for it. Every pass holds the [`FolderGate`](crate::folder_move::FolderGate)
//! like any other write, so it never runs under a folder move, and logs one
//! line per meeting it freed audio in and one summary line. Nothing is shown
//! in the window beyond Settings' "Audio is kept for N days"
//! ([`audio_retention_days`]).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use store::retention::{self, MeetingAudio, Report, Retention};
use tauri::{AppHandle, Manager as _};

use crate::error::UiError;
use crate::lock::lock_or_recover;

/// How long after launch the first pass runs: long enough to stay out of the
/// way of startup.
const FIRST_RUN_DELAY: Duration = Duration::from_secs(30);

/// How often a pass runs after the first.
const RUN_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Managed state: which meetings are being transcribed, and the lock that
/// keeps passes from overlapping.
#[derive(Default)]
pub struct AudioRetention {
    transcribing: Mutex<HashSet<String>>,
    pass: Mutex<()>,
}

/// Marks one meeting as being transcribed, from Stop until its transcript is
/// final. Dropping it ends the mark.
pub struct Transcribing {
    app: AppHandle,
    meeting_id: String,
    /// The transcript never became final: keep the meeting busy until the
    /// app quits rather than delete audio a re-run would need.
    keep: bool,
}

impl Transcribing {
    /// Mark `meeting_id` busy. Call before the recorder goes back to idle, so
    /// there is no moment where the meeting is neither.
    pub fn begin(app: &AppHandle, meeting_id: &str) -> Self {
        if let Some(state) = app.try_state::<AudioRetention>() {
            lock_or_recover(&state.transcribing).insert(meeting_id.to_owned());
        }
        Self {
            app: app.clone(),
            meeting_id: meeting_id.to_owned(),
            keep: false,
        }
    }

    /// The transcript is final (`is_final`), or never will be. When it is,
    /// the mark ends and, with `retention_days: 0`, a pass runs now.
    pub fn settled(mut self, is_final: bool) {
        if !is_final {
            tracing::warn!(
                meeting = %self.meeting_id,
                "the transcript never became final; keeping this meeting's audio"
            );
            self.keep = true;
            return;
        }
        let app = self.app.clone();
        drop(self);
        run_now_if_immediate(&app);
    }
}

impl Drop for Transcribing {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if let Some(state) = self.app.try_state::<AudioRetention>() {
            lock_or_recover(&state.transcribing).remove(&self.meeting_id);
        }
    }
}

/// Start the schedule: a pass [`FIRST_RUN_DELAY`] after launch, then every
/// [`RUN_EVERY`].
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_RUN_DELAY).await;
        let mut every = tokio::time::interval(RUN_EVERY);
        // After a long sleep, one catch-up pass, not a burst of them.
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // The first tick is immediate: that is the launch pass.
            every.tick().await;
            let app = app.clone();
            if let Err(error) = tauri::async_runtime::spawn_blocking(move || run_pass(&app)).await {
                tracing::error!(%error, "the audio retention pass did not finish");
            }
        }
    });
}

/// A notes run ended: with `retention_days: 0`, the meeting it held busy can
/// lose its audio now rather than at the next daily pass.
pub fn after_notes_run(app: &AppHandle) {
    run_now_if_immediate(app);
}

/// Run a pass on a thread of its own when `retention_days` is `0`. The config
/// is read on that thread too: callers may hold a lock.
fn run_now_if_immediate(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-audio-retention".to_owned())
        .spawn(move || {
            if crate::config::audio().retention == Retention::Days(0) {
                run_pass(&app);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the audio retention pass");
    }
}

/// One pass over the meetings folder, through the folder gate.
fn run_pass(app: &AppHandle) {
    let retention = crate::config::audio().retention;
    if retention == Retention::KeepForever {
        tracing::debug!("audio retention is -1: keeping all audio");
        return;
    }
    let Some(state) = app.try_state::<AudioRetention>() else {
        return;
    };
    let _one_at_a_time = lock_or_recover(&state.pass);
    let result = crate::folder_move::writing_in_root(app, |root| {
        run_in(root, retention, SystemTime::now(), &busy_now(app))
            .map_err(|error| UiError::app("retention-failed", error.to_string()))
    });
    if let Err(error) = result {
        // Moving folder, no root, or an unreadable one: the next pass retries.
        tracing::warn!(message = %error.message, "audio retention skipped this run");
    }
}

/// What the app is busy with right now, read from its managed state.
fn busy_now(app: &AppHandle) -> Busy {
    let status = app.state::<crate::recording::Recorder>().status();
    let recording = crate::meetings::Live::from_status(&status);
    let transcribing = app
        .try_state::<AudioRetention>()
        .map(|state| lock_or_recover(&state.transcribing).clone())
        .unwrap_or_default();
    let syncing_tickets = app
        .try_state::<crate::sync::SyncRuns>()
        .map(|runs| runs.running_tickets())
        .unwrap_or_default();
    let notes = app.clone();
    Busy {
        recording: match recording {
            crate::meetings::Live::Meeting(id) => Some(id.to_owned()),
            crate::meetings::Live::Nothing => None,
        },
        transcribing,
        syncing_tickets,
        notes_running: Box::new(move |id| {
            notes
                .try_state::<crate::agent_run::AgentRuns>()
                .is_some_and(|runs| runs.status(id).state == crate::agent_run::State::Running)
        }),
    }
}

/// Everything that makes a meeting busy, gathered before a pass.
pub(crate) struct Busy {
    /// The meeting being recorded (or starting or stopping).
    pub recording: Option<String>,
    /// Meetings stopped whose transcript is not final yet.
    pub transcribing: HashSet<String>,
    /// Ticket ids with a Sync run going.
    pub syncing_tickets: Vec<String>,
    /// Whether a meeting has a notes run going.
    pub notes_running: Box<dyn Fn(&str) -> bool + Send>,
}

impl Busy {
    /// The ids, out of `meetings` under `root`, that must not be touched.
    fn meetings(&self, root: &Path, meetings: &[MeetingAudio]) -> HashSet<String> {
        meetings
            .iter()
            .map(|meeting| meeting.id.as_str())
            .filter(|id| {
                self.recording.as_deref() == Some(*id)
                    || self.transcribing.contains(*id)
                    || (self.notes_running)(id)
                    || self.syncing_in(root, id)
            })
            .map(str::to_owned)
            .collect()
    }

    /// A Sync run is going for one of this meeting's own tickets.
    fn syncing_in(&self, root: &Path, id: &str) -> bool {
        let tickets = root.join(id).join(store::TICKETS_DIR);
        self.syncing_tickets
            .iter()
            .any(|ticket| tickets.join(format!("{ticket}.md")).is_file())
    }
}

/// One pass under `root`: survey, plan, delete, log.
pub(crate) fn run_in(
    root: &Path,
    retention: Retention,
    now: SystemTime,
    busy: &Busy,
) -> Result<Report, store::Error> {
    let meetings = retention::survey(root)?;
    let busy = busy.meetings(root, &meetings);
    let planned = retention::plan(&meetings, now, retention, &busy);
    let report = retention::apply(&planned);
    log(&report, retention, busy.len());
    Ok(report)
}

/// One info line per meeting freed, one per problem, one summary.
fn log(report: &Report, retention: Retention, busy: usize) {
    for (meeting, freed) in report.by_meeting() {
        tracing::info!(
            %meeting,
            files = %freed.files.join(", "),
            bytes = freed.bytes,
            "deleted old meeting audio"
        );
    }
    for path in &report.skipped_locked {
        tracing::info!(path = %path.display(), "audio file in use; will retry next run");
    }
    for failed in &report.errors {
        tracing::warn!(path = %failed.path.display(), error = %failed.error, "could not delete old audio");
    }
    tracing::info!(
        retention_days = retention.as_days(),
        deleted = report.deleted.len(),
        bytes = report.bytes(),
        meetings = report.by_meeting().len(),
        skipped_locked = report.skipped_locked.len(),
        skipped_busy = busy,
        errors = report.errors.len(),
        "audio retention pass done"
    );
}

/// `audio.retention_days` as the app uses it: `-1` keeps audio forever, `0`
/// deletes it once the transcript is done, otherwise the days. A bad value
/// reads as the default 7, as it does for the job itself.
#[tauri::command]
#[specta::specta]
pub async fn audio_retention_days() -> i32 {
    let days = crate::config::audio().retention.as_days();
    i32::try_from(days).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSCRIPT: &str = "[00:00:04] You: Hello.\n";

    fn meeting(root: &Path, id: &str) -> std::path::PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::create_dir_all(dir.join("tickets")).unwrap();
        std::fs::write(dir.join("transcript.md"), TRANSCRIPT).unwrap();
        std::fs::write(dir.join("audio/mic.wav"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("audio/segments.json"), "{}").unwrap();
        dir
    }

    fn idle() -> Busy {
        Busy {
            recording: None,
            transcribing: HashSet::new(),
            syncing_tickets: Vec::new(),
            notes_running: Box::new(|_| false),
        }
    }

    /// Old meetings, by their folder names, in any time zone.
    const IDS: [&str; 5] = [
        "2026-01-01-1000-recording",
        "2026-01-02-1000-transcribing",
        "2026-01-03-1000-notes",
        "2026-01-04-1000-syncing",
        "2026-01-05-1000-idle",
    ];

    #[test]
    fn every_kind_of_busy_meeting_keeps_its_audio() {
        let tmp = tempfile::tempdir().unwrap();
        for id in IDS {
            meeting(tmp.path(), id);
        }
        std::fs::write(
            tmp.path().join(IDS[3]).join("tickets/TICK-0007.md"),
            "---\nid: TICK-0007\n---\n",
        )
        .unwrap();
        let busy = Busy {
            recording: Some(IDS[0].to_owned()),
            transcribing: HashSet::from([IDS[1].to_owned()]),
            syncing_tickets: vec!["TICK-0007".to_owned()],
            notes_running: Box::new(|id| id == IDS[2]),
        };

        let report = run_in(tmp.path(), Retention::Days(0), SystemTime::now(), &busy).unwrap();

        assert_eq!(report.deleted.len(), 1, "{report:?}");
        assert!(!tmp.path().join(IDS[4]).join("audio/mic.wav").exists());
        for id in &IDS[..4] {
            assert!(tmp.path().join(id).join("audio/mic.wav").exists(), "{id}");
        }
        assert!(tmp.path().join(IDS[4]).join("audio/segments.json").exists());
        assert!(tmp.path().join(IDS[4]).join("transcript.md").exists());
    }

    #[test]
    fn keep_forever_and_a_fresh_meeting_delete_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let today = chrono::Local::now().format("%Y-%m-%d-%H%M").to_string();
        meeting(tmp.path(), &format!("{today}-today"));

        for retention in [Retention::KeepForever, Retention::Days(7)] {
            let report = run_in(tmp.path(), retention, SystemTime::now(), &idle()).unwrap();
            assert!(report.deleted.is_empty(), "{retention:?}");
        }
        let report = run_in(tmp.path(), Retention::Days(0), SystemTime::now(), &idle()).unwrap();
        assert_eq!(report.deleted.len(), 1);
    }

    #[test]
    fn a_missing_root_is_an_empty_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let report = run_in(
            &tmp.path().join("nope"),
            Retention::Days(0),
            SystemTime::now(),
            &idle(),
        )
        .unwrap();
        assert_eq!(report, Report::default());
    }
}
