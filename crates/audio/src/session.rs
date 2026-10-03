//! The recording session: a start/tick/stop lifecycle the caller controls,
//! instead of a blocking call bounded by a `--duration`.
//!
//! Moved out of `meet-rec.rs`'s `record()` (TUR-94) so `meet-rec` and, later,
//! the app both drive the same in-process seam rather than risking drift
//! across a subprocess boundary — exactly the question TUR-31 already
//! answered for `stt`. Behaviour is unchanged from the binary's old loop:
//! segment reopen on a device change ([`reopen_segment`]), the drift
//! checkpoints ([`checkpoint`]), the gap padding ([`align_and_pad`]), and the
//! first-position wait all moved verbatim; only the caller-facing shape
//! changed. The segment-opening half now lives in [`segment`], where TUR-87
//! made a slow or silent system track degrade the recording to
//! microphone-only instead of ending it.
//!
//! [`RecordingSession::start`] takes the two [`AudioSource`]s already
//! constructed rather than building them itself, so a caller without real
//! hardware — this module's own tests, chiefly — can hand it a stub and
//! exercise the whole checkpoint/reopen/stop machinery without Core Audio or
//! a microphone TCC grant.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod segment;

use self::segment::{Paths, align_and_pad, reopen_segment, segment_open};
use crate::segments::{self, Anchor, CHECKPOINT_INTERVAL_S, SegmentsWriter};
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
/// the platform choice — the drift this ticket exists to prevent. The choice
/// itself lives in `crate::platform` (SPEC §8.2); under the `stub-audio`
/// feature it is a source that records nothing.
pub fn default_system_source() -> Option<Box<dyn AudioSource>> {
    crate::platform::system_source()
}

/// The microphone [`AudioSource`] for this platform: [`crate::mic::MicSource`],
/// or under the `stub-audio` feature a source that records nothing. What a
/// segment reopen ([`reopen_segment`]) rebuilds the microphone with.
pub fn default_mic_source() -> Box<dyn AudioSource> {
    crate::platform::mic_source()
}

/// §7/§11's checkpoint order: fsync every channel's data, write
/// `segments.json`, then patch every header. Also warns if the tracks drift apart.
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

    let anchor = Anchor {
        mic_host_ns,
        mic_frames,
        sys_host_ns,
        sys_frames,
    };
    segments::rate_guard::warn_if_out_of_step(&writer.last_anchor(), &anchor, sys.is_some());
    writer.update_frames(mic_frames, sys_frames);
    writer.checkpoint_anchor(anchor);
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
    tees: Tees,
    /// `None` on a platform with no device watch yet (`crate::platform`):
    /// [`RecordingSession::tick`] then never sees a change.
    last_output_device: Option<crate::platform::DeviceId>,
    last_input_device: Option<crate::platform::DeviceId>,
}

impl RecordingSession {
    /// Start a session writing `mic.wav`/`system.wav`/`segments.json` into
    /// `dir`, capturing from `mic` and, when present, `sys`.
    ///
    /// `sys` is `None` outright on a platform with no system-audio tap
    /// (SPEC §8.2's Windows stub); when `Some` but that source fails to
    /// start, or starts but delivers no first frame in time (TUR-87), the
    /// session falls back to microphone-only (contract §9: absent
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
        // A system track with no first frame in time is dropped here and the
        // recording goes on microphone-only (TUR-87); only the mic can fail it.
        let start_host_ns = match align_and_pad(&mut *mic, &mut sys) {
            Ok(start) => start,
            Err(e) => {
                segment::stop_quietly(&mut *mic, "microphone after a failed start");
                if let Some(source) = sys.as_deref_mut() {
                    segment::stop_quietly(source, "system audio after a failed start");
                }
                return Err(e);
            }
        };

        let open = segment_open(
            start_host_ns,
            &*mic,
            sys.as_deref(),
            segments::reason::START,
        );
        let writer = SegmentsWriter::new(open);
        // First segments.json write happens at the first checkpoint, not
        // here — nothing has been fsynced yet for it to honestly describe.

        // SPEC §5's AirPods-swap gate: baseline the default devices right
        // after the segment they belong to has already opened, so a swap
        // mid-startup (unlikely, but free to handle correctly) is not
        // mistaken for one that happened during the recording.
        let last_output_device = crate::platform::default_output_device().ok();
        let last_input_device = crate::platform::default_input_device().ok();

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
            last_output_device,
            last_input_device,
        })
    }

    /// [`reopen_segment`] onto the platform's default devices.
    fn reopen(&mut self, reason: &str) -> Result<(), String> {
        let paths = Paths {
            segments: &self.segments_path,
            mic: &self.mic_path,
            sys: &self.sys_path,
        };
        reopen_segment(
            &mut self.mic,
            &mut self.sys,
            &mut self.writer,
            &paths,
            reason,
            &self.tees,
            default_mic_source,
            default_system_source,
        )
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
        // The device reads fail on a platform without a device watch yet
        // (`crate::platform`), so nothing below runs there.
        {
            if let Ok(current) = crate::platform::default_output_device() {
                if self.last_output_device.is_some_and(|prev| prev != current) {
                    tracing::info!("default output device changed — reopening segment");
                    self.reopen(segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED)?;
                    self.last_input_device = crate::platform::default_input_device().ok();
                    self.last_checkpoint = Instant::now();
                }
                self.last_output_device = Some(current);
            }
            if let Ok(current) = crate::platform::default_input_device() {
                if self.last_input_device.is_some_and(|prev| prev != current) {
                    tracing::info!("default input device changed — reopening segment");
                    self.reopen(segments::reason::DEFAULT_INPUT_DEVICE_CHANGED)?;
                    self.last_output_device = crate::platform::default_output_device().ok();
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
