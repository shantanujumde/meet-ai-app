//! The recording session: a start/tick/stop lifecycle the caller controls,
//! instead of a blocking call bounded by a `--duration`.
//!
//! Moved out of `meet-rec.rs`'s `record()` (TUR-94) so `meet-rec` and, later,
//! the app both drive the same in-process seam rather than risking drift
//! across a subprocess boundary — exactly the question TUR-31 already
//! answered for `stt`. Behaviour is unchanged from the binary's old loop:
//! segment reopen on a device change ([`reopen_segment`]), the drift
//! checkpoints ([`checkpoint`]), the gap padding ([`align_and_pad`]), and the
//! first-position wait ([`wait_first_position`]) all moved verbatim; only the
//! caller-facing shape changed.
//!
//! [`RecordingSession::start`] takes the two [`AudioSource`]s already
//! constructed rather than building them itself, so a caller without real
//! hardware — this module's own tests, chiefly — can hand it a stub and
//! exercise the whole checkpoint/reopen/stop machinery without Core Audio or
//! a microphone TCC grant.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::segments::{
    self, Anchor, CHECKPOINT_INTERVAL_S, SAMPLE_RATE_HZ, SegmentOpen, SegmentsWriter,
};
use crate::tee::Tee;
use crate::{AudioSource, Channel, Error as AudioError};

/// The live-transcription copies a session should hand out, one per channel
/// (TUR-31: per channel, never interleaved). Both `None` is capture-only, which
/// is what [`RecordingSession::start`] asks for.
///
/// Held by the session for its whole life rather than handed to the first pair
/// of sources and forgotten: [`reopen_segment`] builds brand-new sources after
/// a device change, and a tee that did not follow them would go quiet at the
/// first AirPods swap without anything failing.
#[derive(Debug, Clone, Default)]
pub struct Tees {
    pub mic: Option<Tee>,
    pub sys: Option<Tee>,
}

impl Tees {
    /// Give `source` the mic tee, if there is one. Returns it for chaining.
    fn attach_mic<'a>(&self, source: &'a mut dyn AudioSource) -> &'a mut dyn AudioSource {
        if let Some(tee) = &self.mic {
            source.tee(tee.clone());
        }
        source
    }

    /// Give `source` the system tee, if there is one. Returns it for chaining.
    fn attach_sys<'a>(&self, source: &'a mut dyn AudioSource) -> &'a mut dyn AudioSource {
        if let Some(tee) = &self.sys {
            source.tee(tee.clone());
        }
        source
    }
}

/// How long to wait for each channel's very first resampled buffer before
/// giving up on head-pad alignment (contract §6) and reporting that channel
/// as broken rather than hanging. Generous relative to the ~50ms warm-path
/// setup Tess measured (TUR-4), because a cold run — first launch after a
/// permission grant — can still show that dialog's latency on top of it.
const FIRST_BUFFER_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a caller should call [`RecordingSession::tick`]. Coarse next to
/// [`CHECKPOINT_INTERVAL_S`] — it only bounds how late a due checkpoint or a
/// default-device change is noticed — and cheap, since a tick with nothing
/// due is two Core Audio property reads.
///
/// Exported (TUR-97) so `meet-rec`'s poll loop and the app's ticker thread
/// share one cadence rather than two copies of `200ms` that could drift, the
/// same reason [`default_system_source`] is shared. With it, a checkpoint
/// lands between `CHECKPOINT_INTERVAL_S` and `CHECKPOINT_INTERVAL_S` +
/// `TICK_INTERVAL` after the previous one.
pub const TICK_INTERVAL: Duration = Duration::from_millis(200);

/// The system-audio [`AudioSource`], where one exists on this platform. `None`
/// on any platform without a process-tap implementation yet (SPEC §8.2's
/// Windows stub) — contract §9's "absent track" path, not an error.
///
/// Exported so `meet-rec` and every future in-process caller (the app,
/// TUR-92) build the same default rather than each growing their own copy of
/// this `cfg` — the drift this ticket exists to prevent.
pub fn default_system_source() -> Option<Box<dyn AudioSource>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(crate::macos::tap::SystemSource::new()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
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

/// Contract §6's head-pad: measure each channel's first resampled buffer,
/// take the earlier one as the segment's `start_host_ns`, and pad whichever
/// channel came up later with that much silence so frame 0 of both channels
/// lands on the same instant.
///
/// Factored out so both the recording's first segment and every later reopen
/// ([`reopen_segment`]) go through the identical alignment logic instead of
/// two copies that could drift apart.
fn align_and_pad(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
) -> Result<u64, String> {
    let mic_first = wait_first_position(mic, FIRST_BUFFER_TIMEOUT)
        .map_err(|e| format!("microphone produced no audio: {e}"))?;
    let sys_first = match sys.as_deref() {
        Some(source) => Some(
            wait_first_position(source, FIRST_BUFFER_TIMEOUT)
                .map_err(|e| format!("system audio produced no audio: {e}"))?,
        ),
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

/// Close the current segment and open a new one, rebuilding whichever
/// channel(s) need a fresh OS-level stream after a default-device change
/// (contract §5/§11's F1; SPEC §5's AirPods-swap gate).
///
/// Always rebuilds *both* channels — a spurious restart on the unaffected
/// channel is the right trade against the alternative (a per-channel
/// segment-relative baseline offset).
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
#[allow(clippy::too_many_arguments)]
fn reopen_segment(
    mic: &mut Box<dyn AudioSource>,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    segments_path: &Path,
    mic_path: &Path,
    sys_path: &Path,
    reason: &str,
    tees: &Tees,
) -> Result<(), String> {
    mic.stop()
        .map_err(|e| format!("stopping microphone for reopen: {e}"))?;
    if let Some(s) = sys.as_mut() {
        s.stop()
            .map_err(|e| format!("stopping system audio for reopen: {e}"))?;
    }

    let mic_close = mic
        .position()
        .ok_or_else(|| "microphone stopped producing audio before a segment reopen".to_string())?;
    let sys_close = match sys.as_deref() {
        Some(s) => s.position().unwrap_or((0, 0)),
        None => (0, 0),
    };

    let mut new_mic: Box<dyn AudioSource> = Box::new(crate::mic::MicSource::new());
    tees.attach_mic(&mut *new_mic);
    new_mic
        .start(mic_path.to_path_buf())
        .map_err(|e| format!("restarting microphone after reopen: {e}"))?;

    let mut new_sys: Option<Box<dyn AudioSource>> = if sys.is_some() {
        match default_system_source() {
            Some(mut source) => match tees.attach_sys(&mut *source).start(sys_path.to_path_buf()) {
                Ok(()) => Some(source),
                Err(e) => {
                    tracing::warn!(
                        "system audio unavailable after reopen ({e}); continuing microphone-only"
                    );
                    None
                }
            },
            None => None,
        }
    } else {
        None
    };

    let new_start_host_ns = align_and_pad(&mut *new_mic, &mut new_sys)?;

    let start_unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .ok();
    let next_open = SegmentOpen {
        start_host_ns: new_start_host_ns,
        start_continuous_ns: None,
        start_unix_ns,
        mic_rate: SAMPLE_RATE_HZ,
        sys_rate: if new_sys.is_some() { SAMPLE_RATE_HZ } else { 0 },
        mic_device_rate: None,
        sys_device_rate: None,
        reason: reason.to_string(),
    };

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
    writer
        .write_atomic(segments_path)
        .map_err(|e| format!("writing segments.json at segment reopen: {e}"))?;

    *mic = new_mic;
    *sys = new_sys;
    Ok(())
}

/// §7/§11's checkpoint order for one mid-recording checkpoint: fsync every
/// channel's data, then let the caller write `segments.json`, then patch
/// every channel's header. This function does steps 1 and 3 and the anchor
/// bookkeeping; the caller supplies the write in between.
fn checkpoint(
    mic: &mut dyn AudioSource,
    sys: &mut Option<Box<dyn AudioSource>>,
    writer: &mut SegmentsWriter,
    segments_path: &Path,
) -> Result<(), String> {
    mic.fsync_data().map_err(|e| format!("mic fsync: {e}"))?;
    if let Some(sys) = sys.as_deref_mut() {
        sys.fsync_data().map_err(|e| format!("system fsync: {e}"))?;
    }

    let (mic_host_ns, mic_frames) = mic
        .position()
        .ok_or_else(|| "microphone stopped producing audio mid-recording".to_string())?;
    let (sys_host_ns, sys_frames) = match sys.as_deref() {
        Some(sys) => sys.position().unwrap_or((0, 0)),
        None => (0, 0),
    };

    writer.update_frames(mic_frames, sys_frames);
    writer.checkpoint_anchor(Anchor {
        mic_host_ns,
        mic_frames,
        sys_host_ns,
        sys_frames,
    });
    writer
        .write_atomic(segments_path)
        .map_err(|e| format!("writing segments.json: {e}"))?;

    mic.patch_header()
        .map_err(|e| format!("mic header patch: {e}"))?;
    if let Some(sys) = sys.as_deref_mut() {
        sys.patch_header()
            .map_err(|e| format!("system header patch: {e}"))?;
    }
    Ok(())
}

fn microphone_error(e: AudioError) -> String {
    match e {
        AudioError::PermissionDenied => {
            "meet-ai does not have permission to record the microphone — grant it in \
             System Settings > Privacy & Security > Microphone"
                .to_string()
        }
        other => format!("microphone: {other}"),
    }
}

/// What [`RecordingSession::status`] reports — "ask it how it is doing"
/// without either side needing to poll the filesystem.
#[derive(Debug, Clone, Copy)]
pub struct SessionStatus {
    /// Wall-clock time since [`RecordingSession::start`] returned.
    pub elapsed: Duration,
    /// Whether the system-audio channel is currently being captured — `false`
    /// from the start on a platform with no tap yet, and also `false` after a
    /// tap that failed to (re)start falls back to microphone-only.
    pub has_system_audio: bool,
}

/// What stopping a session reports back: where the files landed, and whether
/// the system-audio channel was ever part of this recording. Deliberately
/// not frame counts or drift — the role charter says to report what the
/// bytes on disk contain, and the honest way to do that is to read the
/// headers back after the fact, which is a presentation concern for the
/// caller (`meet-rec`'s `report_result`), not the session's.
#[derive(Debug, Clone)]
pub struct StopReport {
    pub mic_path: PathBuf,
    pub sys_path: PathBuf,
    pub segments_path: PathBuf,
    pub has_system_audio: bool,
}

/// A start/tick/stop-controlled capture session: two [`AudioSource`]s writing
/// into one meeting folder, kept aligned and checkpointed, instead of a
/// blocking call bounded by a fixed duration.
///
/// The caller owns the polling cadence: call [`RecordingSession::tick`] on
/// some interval ([`TICK_INTERVAL`], which both `meet-rec`'s loop and the app's
/// ticker thread use) to let device-change detection
/// and periodic checkpoints run, then call [`RecordingSession::stop`] once to
/// finish cleanly. Neither `tick` nor `stop` block waiting for anything beyond
/// the calls `AudioSource` itself makes.
pub struct RecordingSession {
    mic: Box<dyn AudioSource>,
    sys: Option<Box<dyn AudioSource>>,
    writer: SegmentsWriter,
    mic_path: PathBuf,
    sys_path: PathBuf,
    segments_path: PathBuf,
    start_host_ns: u64,
    started: Instant,
    last_checkpoint: Instant,
    /// Kept so [`reopen_segment`] can hand them to the rebuilt sources. Only
    /// macOS watches for device changes today, so only macOS reads it back.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    tees: Tees,
    #[cfg(target_os = "macos")]
    last_output_device: Option<objc2_core_audio::AudioObjectID>,
    #[cfg(target_os = "macos")]
    last_input_device: Option<objc2_core_audio::AudioObjectID>,
}

impl RecordingSession {
    /// Start a session writing `mic.wav`/`system.wav`/`segments.json` into
    /// `dir`, capturing from `mic` and, when present, `sys`.
    ///
    /// `sys` is `None` outright on a platform with no system-audio tap
    /// (SPEC §8.2's Windows stub); when `Some` but that source fails to
    /// start, the session falls back to microphone-only (contract §9: absent
    /// track, `sys_rate` 0 in `segments.json`) rather than failing the whole
    /// recording over a channel that was never required.
    ///
    /// Takes both sources already constructed, rather than building them
    /// itself, so a caller without real hardware — this module's own tests —
    /// can hand it a stub and exercise the checkpoint/reopen/stop machinery
    /// without Core Audio or a microphone TCC grant. `meet-rec` and the app
    /// both build their sources the same way, via [`crate::mic::MicSource`]
    /// and [`default_system_source`], so the two callers cannot quietly
    /// diverge on what "the real recorder" means.
    pub fn start(
        dir: PathBuf,
        mic: Box<dyn AudioSource>,
        sys: Option<Box<dyn AudioSource>>,
    ) -> Result<Self, String> {
        Self::start_with_tees(dir, mic, sys, Tees::default())
    }

    /// [`Self::start`], plus a live copy of each channel's frames for whoever
    /// holds the matching [`crate::tee::TeeFeed`] — the app's live transcript.
    ///
    /// The tees change nothing about the recording: same WAVs, same
    /// `segments.json`, same failure behaviour. A tee whose feed is never read,
    /// or is dropped mid-meeting, costs capture nothing (see [`crate::tee`]).
    pub fn start_with_tees(
        dir: PathBuf,
        mut mic: Box<dyn AudioSource>,
        mut sys: Option<Box<dyn AudioSource>>,
        tees: Tees,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

        let mic_path = dir.join(Channel::Mic.wav_filename());
        let sys_path = dir.join(Channel::System.wav_filename());
        let segments_path = dir.join(meeting_format::layout::SEGMENTS_FILE);

        tracing::info!("starting microphone capture");
        tees.attach_mic(&mut *mic)
            .start(mic_path.clone())
            .map_err(microphone_error)?;

        if let Some(source) = sys.as_mut() {
            tracing::info!("starting system-audio capture");
            if let Err(e) = tees.attach_sys(&mut **source).start(sys_path.clone()) {
                tracing::warn!(
                    "system audio unavailable ({e}); recording microphone only \
                     (contract §9: absent track, sys_rate 0 in segments.json)"
                );
                sys = None;
            }
        } else {
            tracing::info!(
                "no system-audio capture on this platform yet (SPEC §8.2); \
                 recording microphone only"
            );
        }

        // Head-pad (contract §6): align frame 0 of both channels to whichever
        // channel's hardware came up first, by padding the other with silence.
        tracing::info!("measuring channel start alignment");
        let start_host_ns = align_and_pad(&mut *mic, &mut sys)?;

        let start_unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .ok();

        let open = SegmentOpen {
            start_host_ns,
            start_continuous_ns: None,
            start_unix_ns,
            mic_rate: SAMPLE_RATE_HZ,
            sys_rate: if sys.is_some() { SAMPLE_RATE_HZ } else { 0 },
            mic_device_rate: None,
            sys_device_rate: None,
            reason: segments::reason::START.to_string(),
        };
        let writer = SegmentsWriter::new(open);
        // First segments.json write happens at the first checkpoint, not
        // here — nothing has been fsynced yet for it to honestly describe.

        // SPEC §5's AirPods-swap gate: baseline the default devices right
        // after the segment they belong to has already opened, so a swap
        // mid-startup (unlikely, but free to handle correctly) is not
        // mistaken for one that happened during the recording.
        #[cfg(target_os = "macos")]
        let last_output_device = crate::macos::device_watch::default_output_device().ok();
        #[cfg(target_os = "macos")]
        let last_input_device = crate::macos::device_watch::default_input_device().ok();

        Ok(Self {
            mic,
            sys,
            writer,
            mic_path,
            sys_path,
            segments_path,
            start_host_ns,
            started: Instant::now(),
            last_checkpoint: Instant::now(),
            tees,
            #[cfg(target_os = "macos")]
            last_output_device,
            #[cfg(target_os = "macos")]
            last_input_device,
        })
    }

    /// Ask the session how it is doing, without touching disk.
    pub fn status(&self) -> SessionStatus {
        SessionStatus {
            elapsed: self.started.elapsed(),
            has_system_audio: self.sys.is_some(),
        }
    }

    /// Run one iteration of the poll loop: check for a default-device change
    /// (macOS only — SPEC §5's AirPods-swap gate) and, if
    /// [`crate::segments::CHECKPOINT_INTERVAL_S`] has elapsed since the last
    /// one, run an ordinary checkpoint. The caller decides how often to call
    /// this; nothing here sleeps or blocks on a timer of its own.
    pub fn tick(&mut self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            if let Ok(current) = crate::macos::device_watch::default_output_device() {
                if self.last_output_device.is_some_and(|prev| prev != current) {
                    tracing::info!("default output device changed — reopening segment");
                    reopen_segment(
                        &mut self.mic,
                        &mut self.sys,
                        &mut self.writer,
                        &self.segments_path,
                        &self.mic_path,
                        &self.sys_path,
                        segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
                        &self.tees,
                    )?;
                    self.last_input_device =
                        crate::macos::device_watch::default_input_device().ok();
                    self.last_checkpoint = Instant::now();
                }
                self.last_output_device = Some(current);
            }
            if let Ok(current) = crate::macos::device_watch::default_input_device() {
                if self.last_input_device.is_some_and(|prev| prev != current) {
                    tracing::info!("default input device changed — reopening segment");
                    reopen_segment(
                        &mut self.mic,
                        &mut self.sys,
                        &mut self.writer,
                        &self.segments_path,
                        &self.mic_path,
                        &self.sys_path,
                        segments::reason::DEFAULT_INPUT_DEVICE_CHANGED,
                        &self.tees,
                    )?;
                    self.last_output_device =
                        crate::macos::device_watch::default_output_device().ok();
                    self.last_checkpoint = Instant::now();
                }
                self.last_input_device = Some(current);
            }
        }

        if self.last_checkpoint.elapsed() >= Duration::from_secs(CHECKPOINT_INTERVAL_S) {
            checkpoint(
                &mut *self.mic,
                &mut self.sys,
                &mut self.writer,
                &self.segments_path,
            )?;
            self.last_checkpoint = Instant::now();
            tracing::info!("checkpoint at {:.0}s", self.started.elapsed().as_secs_f64());
        }
        Ok(())
    }

    /// Stop cleanly: both channels stopped (WAV headers finalised), a final
    /// anchor latched, and `segments.json` written — contract §7's "equality
    /// on graceful stop", taken only after both streams are fully stopped so
    /// it is exact rather than a checkpoint's necessarily-slightly-behind
    /// approximation.
    pub fn stop(mut self) -> Result<StopReport, String> {
        tracing::info!("stopping");
        self.mic
            .stop()
            .map_err(|e| format!("stopping microphone: {e}"))?;
        if let Some(source) = self.sys.as_mut() {
            source
                .stop()
                .map_err(|e| format!("stopping system audio: {e}"))?;
        }

        let (mic_ns, mic_frames) = self.mic.position().unwrap_or((self.start_host_ns, 0));
        let (sys_ns, sys_frames) = self
            .sys
            .as_deref()
            .and_then(|source| source.position())
            .unwrap_or((0, 0));
        self.writer.update_frames(mic_frames, sys_frames);
        self.writer.checkpoint_anchor(Anchor {
            mic_host_ns: mic_ns,
            mic_frames,
            sys_host_ns: sys_ns,
            sys_frames,
        });
        self.writer
            .write_atomic(&self.segments_path)
            .map_err(|e| format!("writing segments.json: {e}"))?;

        Ok(StopReport {
            mic_path: self.mic_path,
            sys_path: self.sys_path,
            segments_path: self.segments_path,
            has_system_audio: self.sys.is_some(),
        })
    }
}

#[cfg(test)]
mod tests;
