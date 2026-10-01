//! The meeting list, and telling the meeting being recorded from the rest.
//! Also the launch-time pass that makes interrupted recordings playable.

use std::path::Path;

use audio::Channel;
use audio::wav_repair::{classify_audio, expose_unheadered_samples};
use serde::Serialize;

use super::root::root;
use super::view::{MeetingSummary, summarize};
use crate::error::UiError;
use crate::recording::{Phase, Status};

const AUDIO: &str = meeting_format::layout::AUDIO_DIR;
const SEGMENTS: &str = meeting_format::layout::SEGMENTS_FILE;

/// Which meeting, if any, the running app is recording into.
///
/// Built from the recorder's [`Status`] so the list can leave a live meeting
/// alone: mid-recording, its files are indistinguishable from a killed one's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Live<'a> {
    #[default]
    Nothing,
    Meeting(&'a str),
}

impl<'a> Live<'a> {
    pub fn from_status(status: &'a Status) -> Self {
        match (status.phase, status.meeting_id.as_deref()) {
            (Phase::Idle, _) => Live::Nothing,
            (_, Some(id)) => Live::Meeting(id),
            // `Starting` before the recorder has picked an id. It publishes the
            // id before it creates the folder, so no folder can be live yet.
            (_, None) => Live::Nothing,
        }
    }
}

/// The meeting list plus enough context to write honest empty-state copy.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingList {
    pub root: String,
    /// The folder does not exist yet. Before the first recording it never does,
    /// and that is not an error worth alarming anyone about.
    pub root_exists: bool,
    pub meetings: Vec<MeetingSummary>,
}

/// List every meeting folder under the root, newest first.
pub fn list(live: Live<'_>) -> Result<MeetingList, UiError> {
    list_in(&root()?, live)
}

pub(super) fn list_in(root: &Path, live: Live<'_>) -> Result<MeetingList, UiError> {
    if !root.is_dir() {
        return Ok(MeetingList {
            root: root.display().to_string(),
            root_exists: false,
            meetings: Vec::new(),
        });
    }

    // `scan` skips `.app` and every other dot-folder, sorts newest first, and
    // never lets one unreadable folder hide the rest.
    let live_id = live_id(live);
    let meetings = store::folder::scan(root)?
        .iter()
        .map(|folder| {
            let is_live = live_id.as_deref() == Some(folder.id.as_str());
            summarize(folder, is_live)
        })
        .collect();

    Ok(MeetingList {
        root: root.display().to_string(),
        root_exists: true,
        meetings,
    })
}

/// The folder name of the live meeting, if any.
pub(super) fn live_id(live: Live<'_>) -> Option<String> {
    match live {
        Live::Nothing => None,
        Live::Meeting(id) => Some(id.to_string()),
    }
}

/// Make the audio of v0.3.0-era interrupted recordings playable. Run once at
/// launch, before anything can start a recording.
///
/// A recording killed before its first checkpoint — every killed recording
/// on v0.3.0, which never checkpointed — leaves WAVs whose headers still say
/// **0 bytes** of audio over minutes of real samples, and no `segments.json`.
/// Every conforming reader, `read_header_frames` included, trusts the header,
/// so that audio is unreachable: QuickTime plays nothing, and the meeting
/// would claim no audio was kept. TUR-97 rules out a repair step the user has
/// to take, so this one is silent.
///
/// It is deliberately narrow. It only touches a meeting that
///
/// * [`classify_audio`] calls interrupted,
/// * is not the one this app is recording (`live`),
/// * has **no** `segments.json` — the only shape where the header can be
///   behind *all* of the audio. A recording that has checkpointed is already
///   playable to within one checkpoint, and the samples past its header were
///   never accounted for in any `segments.json`; `WavWriter::open_append`
///   deliberately discards exactly those, and this does not second-guess it.
///
/// and in such a meeting it only rewrites the two size fields of a canonical
/// 44-byte header, to cover the whole frames already on disk. It never
/// truncates, moves or reorders a sample, never lowers a size, and a second
/// run finds nothing to do. The meeting stays labelled interrupted afterwards,
/// because it still has no `segments.json`.
///
/// Returns how many WAV headers it rewrote.
pub fn recover_interrupted_audio(live: Live<'_>) -> usize {
    match root() {
        Ok(root) => recover_in(&root, live),
        Err(error) => {
            tracing::warn!(message = %error.message, "no meetings folder to check for interrupted recordings");
            0
        }
    }
}

pub(super) fn recover_in(root: &Path, live: Live<'_>) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let folders = match store::folder::meeting_dirs(root) {
        Ok(dirs) => dirs
            .into_iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (path, name)
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            tracing::warn!(%error, "could not list meetings to check for interrupted recordings");
            return 0;
        }
    };
    let live_id = live_id(live);

    let mut rewritten = 0;
    for (path, name) in &folders {
        if live_id.as_deref() == Some(name.as_str()) {
            continue;
        }
        let audio_dir = path.join(AUDIO);
        if audio_dir.join(SEGMENTS).exists() || classify_audio(&audio_dir).ended_cleanly {
            continue;
        }
        for channel in [Channel::Mic, Channel::System] {
            let wav = audio_dir.join(channel.wav_filename());
            if !wav.is_file() {
                continue;
            }
            match expose_unheadered_samples(&wav) {
                Ok(false) => {}
                Ok(true) => {
                    rewritten += 1;
                    tracing::info!(meeting = %name, file = channel.wav_filename(), "made an interrupted recording's audio playable");
                }
                // Left as found. The meeting still opens and still says it
                // was interrupted; the next launch tries again.
                Err(error) => {
                    tracing::warn!(%error, meeting = %name, file = channel.wav_filename(), "could not make an interrupted recording's audio playable")
                }
            }
        }
    }
    rewritten
}
