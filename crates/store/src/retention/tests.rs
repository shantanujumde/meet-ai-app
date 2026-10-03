//! Fixture meetings on disk, run through `survey`, `plan` and `apply`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;

const TRANSCRIPT: &str = "[00:00:04] Others: Morning everyone.\n";

/// 2026-10-03 12:00 UTC: "now" for every dated fixture.
fn now() -> SystemTime {
    at("2026-10-03T12:00:00+00:00")
}

fn at(rfc3339: &str) -> SystemTime {
    SystemTime::from(chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap())
}

/// A finished meeting folder with every SPEC §3.1 file. `date` goes into
/// `meeting.md` when given; `transcript` into `transcript.md` when given.
fn meeting(root: &Path, id: &str, date: Option<&str>, transcript: Option<&str>) -> PathBuf {
    let dir = root.join(id);
    let audio = dir.join("audio");
    std::fs::create_dir_all(&audio).unwrap();
    std::fs::create_dir_all(dir.join("tickets")).unwrap();
    if let Some(date) = date {
        std::fs::write(
            dir.join("meeting.md"),
            format!("---\nid: {id}\ndate: {date}\n---\n\n## Summary\n"),
        )
        .unwrap();
    }
    if let Some(transcript) = transcript {
        std::fs::write(dir.join("transcript.md"), transcript).unwrap();
    }
    std::fs::write(dir.join("notes.md"), "my notes").unwrap();
    std::fs::write(
        dir.join("tickets").join("TICK-0001.md"),
        "---\nid: TICK-0001\n---\n",
    )
    .unwrap();
    std::fs::write(audio.join("mic.wav"), vec![0u8; 1000]).unwrap();
    std::fs::write(audio.join("system.wav"), vec![0u8; 500]).unwrap();
    std::fs::write(audio.join("segments.json"), r#"{"segments":[]}"#).unwrap();
    dir
}

fn wavs(dir: &Path) -> Vec<PathBuf> {
    vec![dir.join("audio/mic.wav"), dir.join("audio/system.wav")]
}

fn none_busy() -> HashSet<String> {
    HashSet::new()
}

fn plan_root(
    root: &Path,
    now: SystemTime,
    retention: Retention,
    busy: &HashSet<String>,
) -> Vec<PathBuf> {
    let mut planned = plan(&survey(root).unwrap(), now, retention, busy);
    planned.sort();
    planned
}

/// Everything but the WAVs is still there.
fn assert_rest_kept(dir: &Path) {
    for kept in [
        "meeting.md",
        "transcript.md",
        "notes.md",
        "tickets/TICK-0001.md",
        "audio/segments.json",
    ] {
        if kept == "meeting.md" && !dir.join(kept).exists() {
            continue; // fixtures without a date have no meeting.md
        }
        assert!(dir.join(kept).is_file(), "{kept} must survive retention");
    }
    assert!(dir.join("audio").is_dir(), "the audio folder itself stays");
}

#[test]
fn a_meeting_older_than_n_days_loses_its_wavs_and_a_newer_one_keeps_them() {
    let tmp = tempfile::tempdir().unwrap();
    let old = meeting(
        tmp.path(),
        "2026-09-20-1000-old",
        Some("2026-09-20T10:00:00+00:00"),
        Some(TRANSCRIPT),
    );
    let new = meeting(
        tmp.path(),
        "2026-10-01-1000-new",
        Some("2026-10-01T10:00:00+00:00"),
        Some(TRANSCRIPT),
    );

    let planned = plan_root(tmp.path(), now(), Retention::Days(7), &none_busy());
    assert_eq!(planned, wavs(&old));

    let report = apply(&planned);
    assert_eq!(report.deleted.len(), 2);
    assert!(
        report.skipped_locked.is_empty() && report.errors.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.bytes(), 1500);
    for wav in wavs(&old) {
        assert!(!wav.exists(), "{} should be gone", wav.display());
    }
    assert_rest_kept(&old);
    for wav in wavs(&new) {
        assert!(wav.exists(), "{} should be kept", wav.display());
    }
    // A second run has nothing left to do.
    assert!(plan_root(tmp.path(), now(), Retention::Days(7), &none_busy()).is_empty());
}

#[test]
fn exactly_n_days_old_is_kept_and_a_minute_more_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    meeting(
        tmp.path(),
        "2026-09-26-1200-edge",
        Some("2026-09-26T12:00:00+00:00"),
        Some(TRANSCRIPT),
    );
    let past = meeting(
        tmp.path(),
        "2026-09-26-1159-past",
        Some("2026-09-26T11:59:00+00:00"),
        Some(TRANSCRIPT),
    );

    let planned = plan_root(tmp.path(), now(), Retention::Days(7), &none_busy());
    assert_eq!(planned, wavs(&past));
}

#[test]
fn zero_deletes_as_soon_as_the_transcript_is_done() {
    let tmp = tempfile::tempdir().unwrap();
    let just_now = meeting(
        tmp.path(),
        "2026-10-03-1159-call",
        Some("2026-10-03T11:59:00+00:00"),
        Some(TRANSCRIPT),
    );
    // Even one dated after "now" by a wrong clock.
    let future = meeting(
        tmp.path(),
        "2026-10-04-0900-later",
        Some("2026-10-04T09:00:00+00:00"),
        Some(TRANSCRIPT),
    );

    let planned = plan_root(tmp.path(), now(), Retention::Days(0), &none_busy());
    let mut expected = [wavs(&just_now), wavs(&future)].concat();
    expected.sort();
    assert_eq!(planned, expected);

    apply(&planned);
    assert_rest_kept(&just_now);
    assert_rest_kept(&future);
}

#[test]
fn minus_one_keeps_audio_forever() {
    let tmp = tempfile::tempdir().unwrap();
    meeting(
        tmp.path(),
        "2020-01-01-1000-ancient",
        Some("2020-01-01T10:00:00+00:00"),
        Some(TRANSCRIPT),
    );

    assert!(plan_root(tmp.path(), now(), Retention::KeepForever, &none_busy()).is_empty());
}

#[test]
fn a_busy_meeting_is_skipped_however_old() {
    let tmp = tempfile::tempdir().unwrap();
    meeting(
        tmp.path(),
        "2026-09-01-1000-recording",
        Some("2026-09-01T10:00:00+00:00"),
        Some(TRANSCRIPT),
    );
    let idle = meeting(
        tmp.path(),
        "2026-09-02-1000-idle",
        Some("2026-09-02T10:00:00+00:00"),
        Some(TRANSCRIPT),
    );
    let busy = HashSet::from(["2026-09-01-1000-recording".to_owned()]);

    for retention in [Retention::Days(0), Retention::Days(7)] {
        assert_eq!(
            plan_root(tmp.path(), now(), retention, &busy),
            wavs(&idle),
            "{retention:?}"
        );
    }
}

#[test]
fn a_missing_or_empty_transcript_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    meeting(
        tmp.path(),
        "2026-09-01-1000-none",
        Some("2026-09-01T10:00:00+00:00"),
        None,
    );
    meeting(
        tmp.path(),
        "2026-09-01-1100-empty",
        Some("2026-09-01T11:00:00+00:00"),
        Some(""),
    );
    meeting(
        tmp.path(),
        "2026-09-01-1200-blank",
        Some("2026-09-01T12:00:00+00:00"),
        Some(" \n\n\t\n"),
    );

    for retention in [Retention::Days(0), Retention::Days(7)] {
        assert!(
            plan_root(tmp.path(), now(), retention, &none_busy()).is_empty(),
            "{retention:?}"
        );
    }
}

#[test]
fn the_folder_name_dates_a_meeting_with_no_meeting_md() {
    let tmp = tempfile::tempdir().unwrap();
    // A month old in any time zone.
    let old = meeting(
        tmp.path(),
        "2026-09-01-1430-standup",
        None,
        Some(TRANSCRIPT),
    );

    let surveyed = survey_meeting(&old);
    assert!(surveyed.happened_at.is_some());
    assert_eq!(
        plan_root(tmp.path(), now(), Retention::Days(7), &none_busy()),
        wavs(&old)
    );
}

#[test]
fn the_meeting_end_counts_not_its_start() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = meeting(tmp.path(), "2026-09-26-1000-long", None, Some(TRANSCRIPT));
    // Started 7 days 2 hours ago, but ran for 2 days: ended 5 days ago.
    std::fs::write(
        dir.join("meeting.md"),
        "---\ndate: 2026-09-26T10:00:00+00:00\nduration_sec: 172800\n---\n",
    )
    .unwrap();

    assert_eq!(
        survey_meeting(&dir).happened_at,
        Some(at("2026-09-28T10:00:00+00:00"))
    );
    assert!(plan_root(tmp.path(), now(), Retention::Days(7), &none_busy()).is_empty());
}

#[test]
fn a_hand_named_folder_is_dated_by_its_newest_wav() {
    let tmp = tempfile::tempdir().unwrap();
    let old = meeting(tmp.path(), "old notes", None, Some(TRANSCRIPT));
    let fresh = meeting(tmp.path(), "fresh notes", None, Some(TRANSCRIPT));
    let now = SystemTime::now();
    let month_ago = now - Duration::from_secs(30 * 24 * 60 * 60);
    for wav in wavs(&old) {
        std::fs::File::options()
            .write(true)
            .open(&wav)
            .unwrap()
            .set_modified(month_ago)
            .unwrap();
    }

    assert!(survey_meeting(&fresh).happened_at.is_some());
    assert_eq!(
        plan_root(tmp.path(), now, Retention::Days(7), &none_busy()),
        wavs(&old)
    );
}

#[test]
fn a_meeting_whose_age_cannot_be_told_is_kept_unless_retention_is_zero() {
    let meetings = [MeetingAudio {
        id: "x".to_owned(),
        happened_at: None,
        has_transcript: true,
        wavs: vec![PathBuf::from("x/audio/mic.wav")],
    }];
    assert!(plan(&meetings, now(), Retention::Days(7), &none_busy()).is_empty());
    assert_eq!(
        plan(&meetings, now(), Retention::Days(0), &none_busy()).len(),
        1
    );
}

#[test]
fn survey_skips_the_app_folder_and_lists_only_wavs() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join(".app").join("audio");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("x.wav"), "x").unwrap();
    let dir = meeting(tmp.path(), "2026-09-01-1000-a", None, Some(TRANSCRIPT));
    std::fs::write(dir.join("audio").join("notes.txt"), "x").unwrap();
    std::fs::create_dir(dir.join("audio").join("nested.wav")).unwrap();
    std::fs::write(dir.join("audio").join("EXTRA.WAV"), "x").unwrap();

    let surveyed = survey(tmp.path()).unwrap();
    assert_eq!(surveyed.len(), 1);
    assert_eq!(
        surveyed[0].wavs,
        vec![
            dir.join("audio/EXTRA.WAV"),
            dir.join("audio/mic.wav"),
            dir.join("audio/system.wav"),
        ]
    );
    assert!(survey(&tmp.path().join("missing")).unwrap().is_empty());
}

#[test]
fn apply_refuses_anything_but_a_wav_in_an_audio_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = meeting(tmp.path(), "2026-09-01-1000-a", None, Some(TRANSCRIPT));
    let stray = dir.join("stray.wav");
    std::fs::write(&stray, "x").unwrap();
    let refused = [
        dir.join("transcript.md"),
        dir.join("audio/segments.json"),
        stray.clone(),
    ];

    let report = apply(&refused);
    assert!(report.deleted.is_empty());
    assert_eq!(report.errors.len(), 3);
    assert!(stray.exists());
    assert_rest_kept(&dir);
}

#[test]
fn a_file_already_gone_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let report = apply(&[tmp.path().join("m/audio/mic.wav")]);
    assert_eq!(report, Report::default());
}

#[test]
fn the_report_groups_what_was_freed_by_meeting() {
    let tmp = tempfile::tempdir().unwrap();
    let a = meeting(tmp.path(), "2026-09-01-1000-a", None, Some(TRANSCRIPT));
    let b = meeting(tmp.path(), "2026-09-02-1000-b", None, Some(TRANSCRIPT));
    let report = apply(&[wavs(&a), vec![b.join("audio/mic.wav")]].concat());

    let freed = report.by_meeting();
    assert_eq!(freed.len(), 2);
    assert_eq!(freed["2026-09-01-1000-a"].files, ["mic.wav", "system.wav"]);
    assert_eq!(freed["2026-09-01-1000-a"].bytes, 1500);
    assert_eq!(freed["2026-09-02-1000-b"].files, ["mic.wav"]);
    assert_eq!(freed["2026-09-02-1000-b"].bytes, 1000);
}

#[test]
fn retention_days_reads_the_config_values() {
    assert_eq!(Retention::from_days(-1), Some(Retention::KeepForever));
    assert_eq!(Retention::from_days(0), Some(Retention::Days(0)));
    assert_eq!(Retention::from_days(7), Some(Retention::Days(7)));
    assert_eq!(Retention::from_days(-2), None);
    assert_eq!(Retention::from_days(i64::MAX), None);
    assert_eq!(Retention::default(), Retention::Days(7));
    for days in [-1, 0, 30] {
        assert_eq!(Retention::from_days(days).unwrap().as_days(), days);
    }
}

#[test]
fn permission_denied_counts_as_locked_on_every_os() {
    assert!(is_locked(&io::Error::from(io::ErrorKind::PermissionDenied)));
    assert!(!is_locked(&io::Error::from(io::ErrorKind::NotFound)));
    // Windows' sharing (32) and lock (33) violations; elsewhere those numbers
    // are unrelated errors.
    for code in [32, 33] {
        assert_eq!(
            is_locked(&io::Error::from_raw_os_error(code)),
            cfg!(windows)
        );
    }
}

/// A WAV that cannot be deleted right now is skipped, counted, and deleted by
/// the next run. On Unix a read-only `audio/` folder refuses the delete with
/// `PermissionDenied`.
#[cfg(unix)]
#[test]
fn a_locked_wav_is_skipped_and_deleted_on_the_next_run() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = meeting(tmp.path(), "2026-09-01-1000-a", None, Some(TRANSCRIPT));
    let audio = dir.join("audio");
    let writable = std::fs::metadata(&audio).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&audio, read_only).unwrap();

    let planned = plan_root(tmp.path(), now(), Retention::Days(7), &none_busy());
    let report = apply(&planned);
    std::fs::set_permissions(&audio, writable).unwrap();
    if report.deleted.len() == 2 {
        // Running as root, which ignores the folder's mode: nothing to test.
        return;
    }
    assert_eq!(report.skipped_locked, wavs(&dir), "{report:?}");
    assert!(report.errors.is_empty());

    let report = apply(&plan_root(
        tmp.path(),
        now(),
        Retention::Days(7),
        &none_busy(),
    ));
    assert_eq!(report.deleted.len(), 2);
}

/// On Windows, a WAV another program has open without delete sharing is
/// refused with a sharing violation: skipped, counted, deleted next time.
#[cfg(windows)]
#[test]
fn a_locked_wav_is_skipped_and_deleted_on_the_next_run() {
    use std::os::windows::fs::OpenOptionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let dir = meeting(tmp.path(), "2026-09-01-1000-a", None, Some(TRANSCRIPT));
    let held = std::fs::File::options()
        .read(true)
        .share_mode(0)
        .open(dir.join("audio/mic.wav"))
        .unwrap();

    let report = apply(&plan_root(
        tmp.path(),
        now(),
        Retention::Days(7),
        &none_busy(),
    ));
    assert_eq!(
        report.skipped_locked,
        vec![dir.join("audio/mic.wav")],
        "{report:?}"
    );
    assert_eq!(report.deleted.len(), 1);
    assert!(report.errors.is_empty());
    drop(held);

    let report = apply(&plan_root(
        tmp.path(),
        now(),
        Retention::Days(7),
        &none_busy(),
    ));
    assert_eq!(report.deleted.len(), 1);
}
