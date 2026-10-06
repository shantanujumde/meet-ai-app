//! Opening a segment: the first-position wait, the head-pad, and the reopen
//! after a default-device change. Moved out of `session.rs` (TUR-87) when
//! these learned to keep recording the microphone when the system track does
//! not come up.
//!
//! The rule both paths now follow: the system track is optional (contract
//! §9), so nothing about it may end a recording. A tap that starts but whose
//! first frame is late (about 3 s seen on a granted Mac, TUR-72; longer
//! around a Bluetooth A2DP/HFP switch, TUR-80) is stopped and dropped, and
//! the segment carries on microphone-only, exactly as a tap that fails to
//! start already did. Only the microphone can fail a segment, and when it
//! does, every source this reopen started is stopped first, so no worker
//! thread outlives it and every WAV header is patched.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::Tees;
use crate::AudioSource;
use crate::segments::{Anchor, SAMPLE_RATE_HZ, SegmentOpen, SegmentsWriter};

/// How long to wait for each channel's very first resampled buffer before
/// giving up on head-pad alignment (contract §6) rather than hanging: the
/// microphone then fails the segment, and the system track is dropped.
/// Generous relative to the ~50ms warm-path setup Tess measured (TUR-4),
/// because a cold run — first launch after a permission grant — can still
/// show that dialog's latency on top of it.
#[cfg(not(test))]
pub(super) const FIRST_BUFFER_TIMEOUT: Duration = Duration::from_secs(10);
/// Short in tests, so a source that never delivers is given up on quickly.
#[cfg(test)]
pub(super) const FIRST_BUFFER_TIMEOUT: Duration = Duration::from_millis(150);

/// Where one recording's files live.
pub(super) struct Paths<'a> {
    pub segments: &'a Path,
    pub mic: &'a Path,
    pub sys: &'a Path,
}

/// Poll `source.position()` until it reports its first resampled buffer, or
/// give up after `budget`. Busy-polls at 1ms rather than sleeping longer,
/// because the whole point is to catch the *first* buffer as close to its
/// arrival as this process can — sleeping coarsely here would reintroduce the
/// same flush-latency error contract §11 rejects for anchors.
fn wait_first_position(source: &dyn AudioSource, budget: Duration) -> Result<(u64, u64), String> {
    let started = Instant::now();
    loop {
        if let Some(pos) = source.position() {
            return Ok(pos);
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "no audio arrived within {:.1}s of starting capture",
                budget.as_secs_f64()
            ));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn pad_frames_for_gap(gap_ns: u64) -> u64 {
    ((gap_ns as f64 / 1e9) * SAMPLE_RATE_HZ as f64).round() as u64
}

/// Stop a source being given up on, logging rather than returning a failure:
/// the caller is already on its way out, and the stop is what patches the
/// WAV header and ends the worker thread.
pub(super) fn stop_quietly(source: &mut dyn AudioSource, what: &str) {
    if let Err(e) = source.stop() {
        tracing::warn!("could not stop the {what} cleanly: {e}");
    }
}

/// Contract §6's head-pad: measure each channel's first resampled buffer,
/// take the earlier one as the segment's `start_host_ns`, and pad whichever
/// channel came up later with that much silence so frame 0 of both channels
/// lands on the same instant.
///
/// A system source with no first buffer within [`FIRST_BUFFER_TIMEOUT`] is
/// stopped and set to `None`; the segment goes on microphone-only (TUR-87).
/// Only the microphone can make this fail.
///
/// Shared by the recording's first segment and every later reopen
/// ([`reopen_segment`]), so the two cannot align differently.
pub(super) fn align_and_pad(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
) -> Result<u64, String> {
    let mic_first = wait_first_position(mic, FIRST_BUFFER_TIMEOUT)
        .map_err(|e| format!("microphone produced no audio: {e}"))?;
    let sys_first = match sys.as_deref() {
        Some(source) => match wait_first_position(source, FIRST_BUFFER_TIMEOUT) {
            Ok(first) => Some(first),
            Err(e) => {
                tracing::warn!(
                    "system audio produced no audio ({e}); recording microphone only \
                     (contract §9: absent track, sys_rate 0 in segments.json)"
                );
                if let Some(source) = sys.as_deref_mut() {
                    stop_quietly(source, "system audio that never delivered");
                }
                *sys = None;
                None
            }
        },
        None => None,
    };

    let start_host_ns = match sys_first {
        Some((sys_ns, _)) => mic_first.0.min(sys_ns),
        None => mic_first.0,
    };

    if mic_first.0 > start_host_ns {
        let pad = pad_frames_for_gap(mic_first.0 - start_host_ns);
        tracing::info!("padding microphone head with {pad} frames of silence");
        mic.pad_leading_silence(pad)
            .map_err(|e| format!("padding microphone head: {e}"))?;
    }
    if let (Some(source), Some((sys_ns, _))) = (sys.as_deref_mut(), sys_first)
        && sys_ns > start_host_ns
    {
        let pad = pad_frames_for_gap(sys_ns - start_host_ns);
        tracing::info!("padding system-audio head with {pad} frames of silence");
        source
            .pad_leading_silence(pad)
            .map_err(|e| format!("padding system-audio head: {e}"))?;
    }

    Ok(start_host_ns)
}

/// The [`SegmentOpen`] for a segment that has just been aligned, with the
/// device rates each source is resampling from (TUR-87: before this both
/// were always `None`, so a rate bug left no trace in `segments.json`).
pub(super) fn segment_open(
    start_host_ns: u64,
    mic: &dyn AudioSource,
    sys: Option<&dyn AudioSource>,
    reason: &str,
) -> SegmentOpen {
    let start_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .ok();
    SegmentOpen {
        start_host_ns,
        start_continuous_ns: None,
        start_unix_ns,
        mic_rate: SAMPLE_RATE_HZ,
        sys_rate: if sys.is_some() { SAMPLE_RATE_HZ } else { 0 },
        mic_device_rate: mic.device_rate(),
        sys_device_rate: sys.and_then(|source| source.device_rate()),
        reason: reason.to_string(),
    }
}

/// Close the current segment and open a new one, rebuilding whichever
/// channel(s) need a fresh OS-level stream after a default-device change
/// (contract §5/§11's F1; SPEC §5's AirPods-swap gate).
///
/// Always rebuilds *both* channels — a spurious restart on the unaffected
/// channel is the right trade against the alternative (a per-channel
/// segment-relative baseline offset). The new sources come from `new_mic`
/// and `new_sys`, which the session points at the platform defaults and the
/// tests at fakes.
///
/// Stops both channels *first*, then reads their final position — never the
/// other order. `stop()` halts the capture stream and joins its worker
/// thread before its own internal `fsync_data`/`patch_header`, so once it
/// returns, `position()` and the just-patched header are guaranteed to agree
/// exactly. Reading `position()` first and calling `stop()` after would
/// leave a window where the (still-running) worker thread appends more audio
/// that `stop()`'s internal fsync then picks up — so the header would end up
/// declaring more frames than the close anchor this function commits to
/// `segments.json`, reproducing the exact header-ahead-of-segments bug
/// `WavWriter::patch_header`'s `synced_frames` freeze was built to prevent
/// one layer down (TUR-54; `crate::wav_writer`). Caught by running this
/// function against real hardware and checking `drift-check`'s own invariant
/// check, not by inspection.
///
/// The system track never fails this (TUR-87): an old tap that will not
/// stop, a new one that will not start, or one whose first frame is late
/// all leave the next segment microphone-only. A microphone failure returns
/// `Err`, after stopping every source this call started.
///
/// `want_sys` is whether the session asked for system audio at all, not
/// whether the old segment had it: a tap that dropped earlier is rebuilt
/// here at the next reopen (TUR-121).
#[allow(clippy::too_many_arguments)]
pub(super) fn reopen_segment(
    mic: &mut Box<dyn AudioSource>,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    paths: &Paths<'_>,
    reason: &str,
    tees: &Tees,
    want_sys: bool,
    new_mic: impl FnOnce() -> Box<dyn AudioSource>,
    new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
) -> Result<(), String> {
    mic.stop()
        .map_err(|e| format!("stopping microphone for reopen: {e}"))?;
    if let Some(s) = sys.as_mut()
        && let Err(e) = s.stop()
    {
        tracing::warn!("stopping system audio for reopen failed ({e}); trying a fresh tap");
    }

    let mic_close = mic
        .position()
        .ok_or_else(|| "microphone stopped producing audio before a segment reopen".to_string())?;
    let sys_close = match sys.as_deref() {
        Some(s) => s.position().unwrap_or((0, 0)),
        None => (0, 0),
    };

    let mut next_mic = new_mic();
    tees.attach_mic(&mut *next_mic);
    if let Err(e) = next_mic.start(paths.mic.to_path_buf()) {
        stop_quietly(&mut *next_mic, "microphone that failed to restart");
        return Err(format!("restarting microphone after reopen: {e}"));
    }

    let mut next_sys: Option<Box<dyn AudioSource>> = if want_sys {
        match new_sys() {
            Some(mut source) => {
                match tees.attach_sys(&mut *source).start(paths.sys.to_path_buf()) {
                    Ok(()) => Some(source),
                    Err(e) => {
                        tracing::warn!(
                            "system audio unavailable after reopen ({e}); continuing microphone-only"
                        );
                        stop_quietly(&mut *source, "system audio that failed to restart");
                        None
                    }
                }
            }
            None => None,
        }
    } else {
        None
    };

    let new_start_host_ns = match align_and_pad(&mut *next_mic, &mut next_sys) {
        Ok(start) => start,
        Err(e) => {
            stop_quietly(&mut *next_mic, "microphone after a failed reopen");
            if let Some(source) = next_sys.as_deref_mut() {
                stop_quietly(source, "system audio after a failed reopen");
            }
            return Err(e);
        }
    };
    let next_open = segment_open(new_start_host_ns, &*next_mic, next_sys.as_deref(), reason);

    writer.update_frames(mic_close.1, sys_close.1);
    writer.close_segment(
        Anchor {
            mic_host_ns: mic_close.0,
            mic_frames: mic_close.1,
            sys_host_ns: sys_close.0,
            sys_frames: sys_close.1,
        },
        next_open,
    );
    *mic = next_mic;
    *sys = next_sys;
    writer
        .write_atomic(paths.segments)
        .map_err(|e| format!("writing segments.json at segment reopen: {e}"))
}
