//! Turning a recorded meeting folder into `transcript.md`.
//!
//! This is the top of the Phase 1 stack: it takes the two WAVs `meet-rec`
//! wrote (SPEC §3.1), runs them through whichever engine the registry picked,
//! and interleaves the result into one speaker-labelled markdown file.
//!
//! The speaker split is not diarization and does not need to be. L5 locks it
//! to the two channels we already capture separately: `mic.wav` is `You`,
//! `system.wav` is `Others`. Getting it wrong is therefore a file-naming bug,
//! not an accuracy problem.

use std::path::{Path, PathBuf};

use crate::sink::{CollectingSink, MarkdownSink, TranscriptSink};
use crate::{Channel, Error, Speaker, SttEngine, Utterance};

/// The on-disk layout of one meeting, per SPEC §3.1.
#[derive(Debug, Clone)]
pub struct MeetingPaths {
    pub root: PathBuf,
}

impl MeetingPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.root.join("audio")
    }

    pub fn wav(&self, channel: Channel) -> PathBuf {
        self.audio_dir().join(channel.wav_filename())
    }

    pub fn segments_json(&self) -> PathBuf {
        self.audio_dir().join("segments.json")
    }

    pub fn transcript_md(&self) -> PathBuf {
        self.root.join("transcript.md")
    }
}

/// What a transcription run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub transcript_path: PathBuf,
    pub lines: usize,
    /// Which engine ran, for the log line and the task update.
    pub engine: &'static str,
}

/// Transcribe both tracks of a meeting into `transcript.md`.
///
/// Both tracks go through the same engine, then merge by timestamp. They are
/// collected before writing rather than streamed straight to the file because
/// SPEC §3.4 makes `transcript.md` append-only: once a line is written it is
/// never reordered, so the ordering has to be right the first time. The live
/// pane (Phase 2, Nia's [`TranscriptSink`] consumer) is what shows text as it
/// arrives; the file is what has to be correct.
pub fn transcribe_meeting(
    paths: &MeetingPaths,
    engine: &mut dyn SttEngine,
) -> Result<Outcome, Error> {
    let mut utterances = Vec::new();

    for (channel, speaker) in [
        (Channel::Mic, Speaker::You),
        (Channel::System, Speaker::Others),
    ] {
        let wav = paths.wav(channel);
        if !wav.is_file() {
            // A one-sided recording is a real situation — the tap failed, or
            // the user was muted the whole call. Transcribe what exists.
            tracing::warn!(path = %wav.display(), "track missing; skipping");
            continue;
        }

        let mut collected = CollectingSink::new();
        engine.transcribe(&wav, speaker, &mut collected)?;
        tracing::info!(
            engine = engine.name(),
            track = ?channel,
            utterances = collected.utterances.len(),
            "track transcribed"
        );
        utterances.extend(collected.utterances);
    }

    // Stable sort by start time: when both speakers start in the same second,
    // the order the tracks were processed in decides, which is arbitrary but
    // consistent run to run.
    utterances.sort_by_key(|utterance| utterance.start_sec);

    let transcript_path = paths.transcript_md();
    let mut sink = MarkdownSink::create(&transcript_path)?;
    for utterance in &utterances {
        sink.write(utterance)?;
    }
    sink.flush()?;

    Ok(Outcome {
        transcript_path,
        lines: sink.lines_written(),
        engine: engine.name(),
    })
}

/// Transcribe one WAV and return its utterances, without writing anything.
///
/// The building block the app uses when it wants the structured result rather
/// than the file — for example to push lines at the UI over a Tauri channel.
pub fn transcribe_track(
    wav: &Path,
    speaker: Speaker,
    engine: &mut dyn SttEngine,
) -> Result<Vec<Utterance>, Error> {
    let mut sink = CollectingSink::new();
    engine.transcribe(wav, speaker, &mut sink)?;
    Ok(sink.utterances)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine that replays a canned script, so the merge and the file
    /// writing can be tested without any model at all.
    struct ScriptedEngine {
        mic: Vec<(u64, &'static str)>,
        system: Vec<(u64, &'static str)>,
    }

    impl SttEngine for ScriptedEngine {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn transcribe(
            &mut self,
            _wav: &Path,
            speaker: Speaker,
            sink: &mut dyn TranscriptSink,
        ) -> Result<(), Error> {
            let script = match speaker {
                Speaker::You => &self.mic,
                Speaker::Others => &self.system,
            };
            for (start_sec, text) in script {
                sink.write(&Utterance {
                    start_sec: *start_sec,
                    speaker,
                    text: (*text).into(),
                })?;
            }
            sink.flush()
        }
    }

    fn meeting_dir(name: &str) -> MeetingPaths {
        let root =
            std::env::temp_dir().join(format!("meet-ai-transcribe-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("audio")).unwrap();
        // The engine is scripted, so the files only have to exist.
        std::fs::write(root.join("audio/mic.wav"), b"").unwrap();
        std::fs::write(root.join("audio/system.wav"), b"").unwrap();
        MeetingPaths::new(root)
    }

    #[test]
    fn the_two_tracks_interleave_by_timestamp_with_the_right_labels() {
        let paths = meeting_dir("merge");
        let mut engine = ScriptedEngine {
            mic: vec![
                (7, "Sessions are still in memory."),
                (20, "About two days."),
            ],
            system: vec![(1, "Morning everyone."), (14, "How long will that take?")],
        };

        let outcome = transcribe_meeting(&paths, &mut engine).unwrap();
        assert_eq!(outcome.lines, 4);

        let body = std::fs::read_to_string(&outcome.transcript_path).unwrap();
        assert_eq!(
            body,
            "[00:00:01] Others: Morning everyone.\n\
             [00:00:07] You: Sessions are still in memory.\n\
             [00:00:14] Others: How long will that take?\n\
             [00:00:20] You: About two days.\n"
        );

        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn a_silent_meeting_produces_an_empty_transcript_not_a_missing_one() {
        // The whole-stack version of the silence gate: zero lines, but the
        // file still exists so the UI has something to open.
        let paths = meeting_dir("silent");
        let mut engine = ScriptedEngine {
            mic: vec![],
            system: vec![],
        };

        let outcome = transcribe_meeting(&paths, &mut engine).unwrap();
        assert_eq!(outcome.lines, 0);
        assert_eq!(
            std::fs::read_to_string(&outcome.transcript_path).unwrap(),
            ""
        );

        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn a_missing_track_does_not_abort_the_other_one() {
        let paths = meeting_dir("one-sided");
        std::fs::remove_file(paths.wav(Channel::System)).unwrap();

        let mut engine = ScriptedEngine {
            mic: vec![(3, "Can anyone hear me?")],
            system: vec![(1, "this track does not exist")],
        };

        let outcome = transcribe_meeting(&paths, &mut engine).unwrap();
        assert_eq!(outcome.lines, 1);
        let body = std::fs::read_to_string(&outcome.transcript_path).unwrap();
        assert_eq!(body, "[00:00:03] You: Can anyone hear me?\n");

        std::fs::remove_dir_all(&paths.root).ok();
    }

    #[test]
    fn the_meeting_layout_matches_the_spec() {
        let paths = MeetingPaths::new("/tmp/2026-09-01-1430-standup");
        assert!(paths.wav(Channel::Mic).ends_with("audio/mic.wav"));
        assert!(paths.wav(Channel::System).ends_with("audio/system.wav"));
        assert!(paths.segments_json().ends_with("audio/segments.json"));
        assert!(paths.transcript_md().ends_with("transcript.md"));
    }
}
