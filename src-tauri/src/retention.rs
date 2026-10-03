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
//!
//! TUR-85: two more things keep audio, both on disk so they survive a
//! relaunch. A meeting whose recording was cut short (the list's
//! "Interrupted", `audio::wav_repair::classify_audio`), and one still marked
//! `audio/.incomplete`: the marker is written with the meeting folder and
//! removed here ([`transcript_finished`]) only when live transcription ended
//! cleanly, with no failure and no timeout. And when `config.jsonc` cannot be
//! read or parsed, or its `audio` section is not valid, no pass runs at all
//! (logged, and Settings says "Audio cleanup paused").

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use store::retention::{self, MeetingAudio, Report, Retention};
use tauri::{AppHandle, Manager as _};

use crate::config::RetentionPolicy;
use crate::error::UiError;
use crate::live_transcript;
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

/// Live transcription of the meeting in `meeting_dir` is over, with
/// `status`. Only a clean end (`stopped`: no failure, no timeout) removes the
/// `audio/.incomplete` marker and so lets retention delete its audio later.
pub fn transcript_finished(meeting_dir: &Path, status: &live_transcript::Status) {
    if status.state != live_transcript::State::Stopped {
        tracing::warn!(
            meeting = %meeting_dir.display(),
            state = ?status.state,
            "the transcript is not complete; keeping this meeting's audio"
        );
        return;
    }
    if let Err(error) = retention::mark_complete(meeting_dir) {
        // Left marked, so kept: the safe way to be wrong.
        tracing::warn!(%error, meeting = %meeting_dir.display(), "could not mark the transcript complete");
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
            if crate::config::audio() == RetentionPolicy::Run(Retention::Days(0)) {
                run_pass(&app);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the audio retention pass");
    }
}

/// One pass over the meetings folder, through the folder gate.
fn run_pass(app: &AppHandle) {
    // A pause is logged by `config::audio` itself.
    let policy = crate::config::audio();
    match policy {
        RetentionPolicy::Pause(_) => return,
        RetentionPolicy::Run(Retention::KeepForever) => {
            tracing::debug!("audio retention is -1: keeping all audio");
            return;
        }
        RetentionPolicy::Run(Retention::Days(_)) => {}
    }
    let Some(state) = app.try_state::<AudioRetention>() else {
        return;
    };
    let _one_at_a_time = lock_or_recover(&state.pass);
    let result = crate::folder_move::writing_in_root(app, |root| {
        pass_in(root, &policy, SystemTime::now(), &busy_now(app))
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

/// One pass under `root` if `policy` allows it; `None` (nothing read or
/// deleted) when it is paused.
pub(crate) fn pass_in(
    root: &Path,
    policy: &RetentionPolicy,
    now: SystemTime,
    busy: &Busy,
) -> Result<Option<Report>, store::Error> {
    match policy {
        RetentionPolicy::Run(retention) => run_in(root, *retention, now, busy).map(Some),
        RetentionPolicy::Pause(_) => Ok(None),
    }
}

/// One pass under `root`: survey, plan, delete, log.
pub(crate) fn run_in(
    root: &Path,
    retention: Retention,
    now: SystemTime,
    busy: &Busy,
) -> Result<Report, store::Error> {
    // The meeting list's own rule for Interrupted (TUR-97).
    let ended_cleanly = |audio: &Path| audio::wav_repair::classify_audio(audio).ended_cleanly;
    let meetings = retention::survey(root, &ended_cleanly)?;
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

/// What Settings' retention line says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AudioRetentionSetting {
    /// The job runs. `days`: `-1` keeps audio forever, `0` deletes it once
    /// the transcript is done, otherwise the days.
    Running { days: i32 },
    /// No audio is deleted: `config.jsonc` could not be read or parsed, or
    /// its `audio` section is not valid. `reason` says which.
    Paused { reason: String },
}

impl From<RetentionPolicy> for AudioRetentionSetting {
    fn from(policy: RetentionPolicy) -> Self {
        match policy {
            RetentionPolicy::Run(retention) => Self::Running {
                days: i32::try_from(retention.as_days()).unwrap_or(i32::MAX),
            },
            RetentionPolicy::Pause(reason) => Self::Paused { reason },
        }
    }
}

/// `audio.retention_days` as the retention job reads it, for Settings.
#[tauri::command]
#[specta::specta]
pub async fn audio_retention_days() -> AudioRetentionSetting {
    crate::config::audio().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use audio::segments::{Anchor, SegmentOpen, SegmentsWriter};
    use audio::wav_writer::WavWriter;

    const TRANSCRIPT: &str = "[00:00:04] You: Hello.\n";

    /// One second of the recorder's 16 kHz mono.
    const SECOND: u64 = 16_000;

    /// A meeting whose recording was stopped with Stop and whose live
    /// transcript ended cleanly: written by the recorder's own writers, the
    /// way `RecordingSession::stop` leaves it, with no `audio/.incomplete`.
    fn meeting(root: &Path, id: &str) -> std::path::PathBuf {
        let dir = root.join(id);
        let audio = dir.join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::create_dir_all(dir.join("tickets")).unwrap();
        std::fs::write(dir.join("transcript.md"), TRANSCRIPT).unwrap();
        let mut wav = WavWriter::create(&audio.join("mic.wav")).unwrap();
        wav.append(&vec![7i16; SECOND as usize]).unwrap();
        wav.fsync_data().unwrap();
        wav.patch_header().unwrap();
        segments_json(&audio, SECOND);
        dir
    }

    fn segments_json(audio: &Path, mic_frames: u64) {
        let mut writer = SegmentsWriter::new(SegmentOpen {
            start_host_ns: 1_000_000_000,
            start_continuous_ns: Some(1_000_000_000),
            start_unix_ns: None,
            mic_rate: 16_000,
            sys_rate: 0,
            mic_device_rate: None,
            sys_device_rate: None,
            reason: audio::segments::reason::START.into(),
        });
        writer.update_frames(mic_frames, 0);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 2_000_000_000,
            mic_frames,
            sys_host_ns: 2_000_000_000,
            sys_frames: 0,
        });
        writer.write_atomic(&audio.join("segments.json")).unwrap();
    }

    /// A meeting `kill -9`'d mid-recording, after lines had settled: the
    /// folder as `create_meeting_folder` made it (marker included), samples
    /// past a header never patched, and no `segments.json`.
    fn killed(root: &Path, id: &str) -> std::path::PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        store::retention::mark_incomplete(&dir).unwrap();
        std::fs::write(dir.join("transcript.md"), TRANSCRIPT).unwrap();
        let mut wav = WavWriter::create(&dir.join("audio/mic.wav")).unwrap();
        wav.append(&vec![7i16; SECOND as usize]).unwrap();
        dir
    }

    fn status(state: live_transcript::State) -> live_transcript::Status {
        live_transcript::Status {
            state,
            engine: Some("apple-speech".to_owned()),
            detail: None,
        }
    }

    fn mic(dir: &Path) -> std::path::PathBuf {
        dir.join("audio/mic.wav")
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

    // --- TUR-85 -------------------------------------------------------------

    #[test]
    fn a_clean_meeting_loses_its_audio_after_n_days_and_not_before() {
        let tmp = tempfile::tempdir().unwrap();
        let old = meeting(tmp.path(), "2026-01-01-1000-old");
        let today = chrono::Local::now().format("%Y-%m-%d-%H%M").to_string();
        let fresh = meeting(tmp.path(), &format!("{today}-fresh"));

        let report = run_in(tmp.path(), Retention::Days(7), SystemTime::now(), &idle()).unwrap();

        assert_eq!(report.deleted.len(), 1, "{report:?}");
        assert!(!mic(&old).exists());
        assert!(mic(&fresh).exists());
        assert!(old.join("audio/segments.json").exists());
    }

    #[test]
    fn an_interrupted_meeting_keeps_its_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = killed(tmp.path(), "2026-01-01-1000-killed");
        // Even with the marker gone, the recording itself says Interrupted.
        store::retention::mark_complete(&dir).unwrap();
        assert!(!audio::wav_repair::classify_audio(&dir.join("audio")).ended_cleanly);

        for retention in [Retention::Days(0), Retention::Days(7)] {
            let report = run_in(tmp.path(), retention, SystemTime::now(), &idle()).unwrap();
            assert!(report.deleted.is_empty(), "{retention:?}: {report:?}");
        }
        assert!(mic(&dir).exists());
    }

    #[test]
    fn a_failed_or_timed_out_live_transcription_keeps_the_audio() {
        let tmp = tempfile::tempdir().unwrap();
        // Stopped cleanly with Stop, but transcription failed mid-meeting (or
        // Stop's wait timed out): `finish_final` reports `failed`. The lines
        // that settled are in transcript.md.
        let partial = meeting(tmp.path(), "2026-01-01-1000-partial");
        store::retention::mark_incomplete(&partial).unwrap();
        transcript_finished(&partial, &status(live_transcript::State::Failed));
        assert!(partial.join("audio/.incomplete").exists());

        let report = run_in(tmp.path(), Retention::Days(0), SystemTime::now(), &idle()).unwrap();
        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(mic(&partial).exists());
    }

    #[test]
    fn a_clean_final_transcript_lets_the_audio_go() {
        let tmp = tempfile::tempdir().unwrap();
        let done = meeting(tmp.path(), "2026-01-01-1000-done");
        store::retention::mark_incomplete(&done).unwrap();

        for not_clean in [
            live_transcript::State::Idle,
            live_transcript::State::Running,
        ] {
            transcript_finished(&done, &status(not_clean));
            assert!(done.join("audio/.incomplete").exists(), "{not_clean:?}");
        }
        transcript_finished(&done, &status(live_transcript::State::Stopped));
        assert!(!done.join("audio/.incomplete").exists());

        let report = run_in(tmp.path(), Retention::Days(0), SystemTime::now(), &idle()).unwrap();
        assert_eq!(report.deleted.len(), 1, "{report:?}");
    }

    #[test]
    fn a_crash_then_a_relaunch_keeps_the_audio() {
        // The app died mid-recording, so nothing in memory survives: the
        // launch pass sees an idle app and, with `0`, deletes whatever it may.
        let tmp = tempfile::tempdir().unwrap();
        let crashed = killed(tmp.path(), "2026-01-01-1000-crashed");
        // A crash that landed where the WAVs still look finished (TUR-97's
        // blind spot) is kept by the marker alone.
        let blind = meeting(tmp.path(), "2026-01-02-1000-blind");
        store::retention::mark_incomplete(&blind).unwrap();

        let report = run_in(tmp.path(), Retention::Days(0), SystemTime::now(), &idle()).unwrap();

        assert!(report.deleted.is_empty(), "{report:?}");
        assert!(mic(&crashed).exists());
        assert!(mic(&blind).exists());
    }

    #[test]
    fn an_unreadable_or_invalid_config_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Meetings");
        let old = meeting(&root, "2026-01-01-1000-old");
        let config = tmp.path().join("config.jsonc");

        // Unreadable: something in the file's place that cannot be read.
        std::fs::create_dir(&config).unwrap();
        let unreadable = crate::config::retention_policy_at(&config);
        std::fs::remove_dir(&config).unwrap();
        let mut paused = vec![unreadable];
        for raw in [
            "{ not json",
            r#"{ "audio": { "retention_days": -1 }, "agent": { , } }"#,
            r#"{ "audio": { "retention_days": -5 } }"#,
            r#"{ "audio": { "retention_days": "7" } }"#,
            r#"{ "audio": null }"#,
        ] {
            std::fs::write(&config, raw).unwrap();
            paused.push(crate::config::retention_policy_at(&config));
        }

        for policy in &paused {
            assert!(matches!(policy, RetentionPolicy::Pause(_)), "{policy:?}");
            let report = pass_in(&root, policy, SystemTime::now(), &idle()).unwrap();
            assert_eq!(report, None, "{policy:?}");
            assert!(mic(&old).exists(), "{policy:?}");
            assert!(matches!(
                AudioRetentionSetting::from(policy.clone()),
                AudioRetentionSetting::Paused { .. }
            ));
        }
    }

    #[test]
    fn an_absent_config_or_key_keeps_audio_for_seven_days() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Meetings");
        let old = meeting(&root, "2026-01-01-1000-old");
        let config = tmp.path().join("config.jsonc");

        let absent_file = crate::config::retention_policy_at(&config);
        std::fs::write(&config, r#"{ "audio": { "warn_no_headphones": true } }"#).unwrap();
        let absent_key = crate::config::retention_policy_at(&config);

        for policy in [absent_file, absent_key] {
            assert_eq!(policy, RetentionPolicy::Run(Retention::Days(7)));
            assert_eq!(
                AudioRetentionSetting::from(policy),
                AudioRetentionSetting::Running { days: 7 }
            );
        }
        let report = pass_in(
            &root,
            &RetentionPolicy::Run(Retention::Days(7)),
            SystemTime::now(),
            &idle(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.deleted.len(), 1, "{report:?}");
        assert!(!mic(&old).exists());
    }

    #[test]
    fn the_settings_line_crosses_the_wire_tagged() {
        let running = serde_json::to_value(AudioRetentionSetting::Running { days: -1 }).unwrap();
        assert_eq!(
            running,
            serde_json::json!({ "state": "running", "days": -1 })
        );
        let paused = serde_json::to_value(AudioRetentionSetting::Paused {
            reason: "config.jsonc: x".to_owned(),
        })
        .unwrap();
        assert_eq!(
            paused,
            serde_json::json!({ "state": "paused", "reason": "config.jsonc: x" })
        );
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
