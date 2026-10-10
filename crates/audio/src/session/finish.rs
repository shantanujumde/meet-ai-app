//! Ending a segment in §7's checkpoint order (TUR-162): stop and fsync both
//! channels, write `segments.json`, and only then patch the WAV headers.
//!
//! A stop, a pause and the first half of a reopen all end a segment. They
//! used to call each source's full `stop`, which patches the header to every
//! frame it appended, and write `segments.json` afterwards: for a reopen, only
//! once the new sources had started and aligned, which can take half a
//! minute. A `kill -9` in that window (SPEC's own AirPods swap gate) left
//! headers declaring frames no `segments.json` covered, the one direction
//! readers cannot recover from. Here, as in a checkpoint, the header never
//! gets ahead: a crash at any step leaves it at or behind `segments.json`.

use std::path::Path;

use crate::AudioSource;
use crate::segments::{Anchor, SegmentsWriter};

/// How a segment ended, when the microphone side of it worked.
pub(super) struct Ended {
    /// Where each channel ended: the segment's last anchor, now on disk.
    pub close: Anchor,
    /// Why the system track could not be finished, if it could not. Its
    /// `segments.json` frames then stop at the last checkpoint, which is
    /// durable, and its header is left there too.
    pub sys_error: Option<String>,
}

/// End the current segment of `writer`: stop capture on both channels and
/// fsync each ([`AudioSource::stop_capture`]), latch where each ended as the
/// segment's last anchor, write `segments.json` to `segments_path`, then
/// patch both headers. `why` names the caller in errors.
///
/// Both channels are always stopped, whatever fails. A channel whose fsync
/// failed is claimed only up to the last checkpoint (the last anchor, whose
/// frames were fsynced before it was taken) and its header is not patched.
/// A microphone failure, or a failed `segments.json` write, is the `Err`,
/// returned after everything else here has been done; a system failure is
/// reported in [`Ended::sys_error`] for the caller to weigh.
pub(super) fn end_segment(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    segments_path: &Path,
    why: &str,
) -> Result<Ended, String> {
    let last = writer.last_anchor();

    // 1. Stop and fsync. `position()` is final once each returns.
    let mic_stopped = mic
        .stop_capture()
        .map_err(|e| format!("stopping microphone for {why}: {e}"));
    let sys_stopped = sys.as_deref_mut().map(|s| s.stop_capture());

    let (mic_host_ns, mic_frames) = match &mic_stopped {
        Ok(()) => mic.position(),
        Err(_) => None,
    }
    .unwrap_or((last.mic_host_ns, last.mic_frames));
    let (sys_host_ns, sys_frames) = match (sys.as_deref(), &sys_stopped) {
        (Some(s), Some(Ok(()))) => s.position().unwrap_or((0, 0)),
        (Some(_), Some(Err(_))) => (last.sys_host_ns, last.sys_frames),
        _ => (0, 0),
    };
    let close = Anchor {
        mic_host_ns,
        mic_frames,
        sys_host_ns,
        sys_frames,
    };

    // 2. `segments.json`. A segment ended twice (a stop after a pause, or
    // after a failed reopen) ends where it did: no second, identical anchor.
    writer.update_frames(mic_frames, sys_frames);
    if writer.last_anchor() != close {
        writer.checkpoint_anchor(close);
    }
    let written = writer
        .write_atomic(segments_path)
        .map_err(|e| format!("writing segments.json at {why}: {e}"));

    // 3. The headers, only once `segments.json` covers what they declare,
    // and only for a channel whose fsync worked.
    let mut sys_error = match sys_stopped {
        Some(Err(e)) => Some(e.to_string()),
        _ => None,
    };
    let mut mic_result = mic_stopped;
    if written.is_ok() {
        if mic_result.is_ok() {
            mic_result = mic
                .patch_header()
                .map_err(|e| format!("microphone header patch at {why}: {e}"));
        }
        if let (Some(s), None) = (sys.as_deref_mut(), &sys_error)
            && let Err(e) = s.patch_header()
        {
            sys_error = Some(format!("header patch: {e}"));
        }
    }

    mic_result?;
    written?;
    Ok(Ended { close, sys_error })
}
