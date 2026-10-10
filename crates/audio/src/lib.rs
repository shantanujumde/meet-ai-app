//! Audio capture for meet-ai.
//!
//! Phase 0 territory (SPEC §5). Both [`AudioSource`] implementations are real:
//! [`mic::MicSource`] and the Core Audio process tap,
//! [`macos::tap::SystemSource`]. Also real: the on-disk contract
//! ([`segments`]), the crash-safe writer that produces the WAV half of it
//! ([`wav_writer`]), the permission chime and its detector ([`chime`]), the
//! measurement built on top of it ([`permission_check`]), and the device-rate
//! resampler ([`resample`]).
//!
//! [`AudioSource`] is one trait implemented per platform (SPEC §4) so the
//! Windows port (SPEC §8.2) is additive rather than a rewrite. macOS, Windows
//! and Linux each have a side under `platform/`; the macOS tap is the only one
//! that needs a signed build and the OS audio permission to run for real.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;
use std::time::Duration;

/// How long any Core Audio call that can trigger a TCC consent dialog — the
/// process tap, and `cpal`'s microphone stream creation — is allowed to block
/// before whichever caller is timing it gives up on it.
///
/// These calls block for as long as a human takes to answer the dialog, not
/// for how long the underlying API needs: Tess measured real dialogs at
/// 1393 ms and 2059 ms, against 5.7 ms warm with no dialog shown (TUR-4,
/// `spikes/phase0a-tcc`). A "safe-looking" 1 s guard would abort the one call
/// that is about to succeed, and only on the very first run — every later run
/// is warm and the bug is invisible in testing.
///
/// ⚠️ This constant is *not*, by itself, enough to bound the call. Measured
/// directly on this machine (TUR-4): in an environment with nobody available
/// to answer the dialog — an agent session with no display/Accessibility
/// access, or headless CI — `cpal`'s coreaudio backend does not error, it
/// blocks `build_input_stream` *forever*; passing this value as that call's
/// own `timeout` argument does nothing, because (per `cpal` 0.18.2's source)
/// that parameter is only read by a sample-rate-negotiation fallback path,
/// never by the call that actually blocks on permission
/// (`AudioUnitInitialize`). [`mic::MicSource::start`] is what makes the
/// *caller* time-bounded instead: it runs the blocking call on its own
/// thread and gives up on that thread with `recv_timeout(AUDIO_PERMISSION_TIMEOUT)`,
/// leaking the still-blocked thread rather than joining it. The tap's future
/// call site should do the same — do not assume passing this to a Core Audio
/// API is sufficient on its own.
pub const AUDIO_PERMISSION_TIMEOUT: Duration = Duration::from_secs(30);

/// The OS seam (SPEC §8.2). The only module that names an operating system:
/// it routes to `src/macos/` (SPEC §4 ⛔: OS-specific code lives there and
/// nowhere else), or to the Windows and Linux stubs, and the rest of the crate
/// calls `platform::…` with no `cfg` of its own.
mod platform;

/// `audio::macos` on macOS: the capture implementation, re-exported from
/// `platform` so its public path is unchanged.
// Empty, so unused, on every other OS.
#[allow(unused_imports)]
pub use platform::public::*;

/// The recording-start chime and its detector (SPEC §8.1, decided in A6).
/// Platform-agnostic for the same reason `segments` is: it is the contract
/// between the side that plays the chime and the side that looks for it, and
/// both ends have to agree on it exactly.
pub mod chime;

/// The `segments.json` contract and the drift maths that reads it (SPEC §3.4,
/// amended by A5). Platform-agnostic on purpose: it is a file format, and
/// `drift-check` has to parse it wherever a recording is read.
pub mod segments;

/// The crash-safe incremental WAV writer (contract §7, §13). Platform-agnostic:
/// it only knows about bytes and file offsets, never about Core Audio.
pub mod wav_writer;

/// Finished-vs-interrupted classification and header repair for a recording's WAVs.
pub mod wav_repair;

/// Device-rate → 16 kHz mono resampling (SPEC §2.3). Platform-agnostic: `cpal`
/// and the Core Audio tap both hand back PCM at whatever rate the device
/// negotiated, and this is the one place that brings it to the rate every WAV
/// and every anchor formula assumes.
pub(crate) mod resample;

/// Measuring the rate a device really delivers against the host clock
/// (TUR-84), for the tap and the microphone alike (TUR-87).
pub(crate) mod rate_meter;

/// Raw device samples to 16 kHz mono, following the measured rate.
pub(crate) mod pipeline;

/// Whether the default mic and speakers are in use by anyone (TUR-31), on
/// every platform: [`activity::device_activity`].
pub mod activity;

/// Which apps are using a mic, by name, never meet-ai itself (TUR-142), on
/// every platform: [`mic_users::mic_users`].
pub mod mic_users;

/// The microphone [`AudioSource`], via `cpal`. Cross-platform on purpose —
/// unlike the process tap, `cpal` already runs on Windows, so this is not
/// gated under `macos`.
pub mod mic;

/// Which microphone a recording opens: the Mac's own when the default is
/// Bluetooth, so headphones stay in playback mode (TUR-91).
pub mod mic_choice;

/// Whether the default output is speakers or headphones, for the "No
/// headphones" warning (TUR-65, SPEC L6).
pub mod headphones;

/// System audio from an output device's loopback (TUR-37): the shared gap
/// filling, clock, silent-buffer and device-switch logic, and the
/// [`AudioSource`] built on them. Windows' WASAPI backend is in `platform`.
pub mod loopback;

/// One channel's WAV writer and latched position, shared by the microphone
/// and the loopback source so the two cannot write a track differently.
pub(crate) mod track;

/// When each written frame was captured: the capture time each packet
/// carries through the ring, and the worker's feed into [`track`] (TUR-151).
pub(crate) mod capture_clock;

/// A source's worker thread, stopped and joined on drop (TUR-162).
pub(crate) mod worker;

/// The bounded wait on a build that can block on a TCC dialog, and the
/// teardown of one that finishes late (TUR-162).
pub(crate) mod init_thread;

/// The real audio-permission measurement: runs the [`chime`] positive control
/// against a live [`macos::tap::SystemSource`], and a start/stop probe against
/// [`mic::MicSource`]. This is the "measurement" `src-tauri/src/permission.rs`
/// defers to — see that module's doc comment.
pub mod permission_check;

/// The capture session: a start/tick/stop lifecycle over two [`AudioSource`]s
/// (SPEC §5, TUR-94). `meet-rec` and the app both drive one of these rather
/// than each owning their own copy of the checkpoint/reopen loop.
pub mod session;

/// A second copy of each channel's 16 kHz frames, fed alongside the WAV, for
/// live transcription (TUR-31's stdin decision; wired into the app by TUR-96).
/// Platform-agnostic: it is a bounded queue and a gap counter, nothing more.
pub mod tee;

/// Which side of the conversation a stream came from (L5).
///
/// Defined once in `meeting-format` and re-exported here, so the channel this
/// crate records and the channel `stt` transcribes are the same type — not two
/// enums kept in step by hand.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::Channel;

/// A platform's implementation of one capture channel.
///
/// Implementations are expected to own their own OS threads. Audio callbacks
/// must never run on the tokio runtime (SPEC §2.3).
///
/// `position` and `checkpoint` exist for the orchestrator (`meet-rec`'s main
/// loop) to build `segments.json`'s checkpoint [`crate::segments::Anchor`]s
/// (contract revision 3, §11) without the WAV writer itself living outside
/// this trait. Neither is real-time-safe to call from an audio callback —
/// both lock a mutex a worker thread also holds — and neither is meant to be:
/// the orchestrator calls them from its own checkpoint thread, on the
/// `CHECKPOINT_INTERVAL_S` cadence (`crate::segments::CHECKPOINT_INTERVAL_S`).
pub trait AudioSource: Send {
    /// Start writing 16 kHz mono PCM to `dest`, returning once capture is live.
    fn start(&mut self, dest: PathBuf) -> Result<(), Error>;

    /// Stop capture and flush the WAV header.
    fn stop(&mut self) -> Result<(), Error>;

    /// The first half of [`AudioSource::stop`]: stop capture, wait for the
    /// worker to write its last frame, and fsync the data, but leave the
    /// header alone. [`AudioSource::position`] is final once it returns.
    /// Calling it again is harmless.
    ///
    /// The recording session ends a segment with this, writes
    /// `segments.json`, and only then calls [`AudioSource::patch_header`]:
    /// §7's checkpoint order (TUR-162). A full `stop` there would patch the
    /// header first, and a crash before `segments.json` caught up would leave
    /// a header declaring frames no `segments.json` accounts for.
    ///
    /// The default is `stop`, which keeps the old order: fine for the
    /// no-op stub and the test fixtures, wrong for anything that records.
    /// Every real source overrides it.
    fn stop_capture(&mut self) -> Result<(), Error> {
        self.stop()
    }

    /// Which channel this source feeds.
    fn channel(&self) -> Channel;

    /// The most recent (host_ns, frames) pair this channel has written:
    /// `frames` 16 kHz frames are in this segment, and `host_ns` is when
    /// frame `frames` (the next one) was captured, so frame 0 was captured
    /// `frames / 16 kHz` before it (TUR-151: before, it was the latest
    /// callback's time, ahead of the frames by the ring's backlog and the
    /// resampler's pending input). `None` before the first resampled chunk
    /// has been written.
    ///
    /// This is deliberately *not* "frames flushed so far" sampled at whatever
    /// instant the orchestrator happens to call this — contract §11 is
    /// explicit that anchoring on flush position measures the writer's own
    /// latency instead of the device clock. Implementations must latch both
    /// halves of the pair together, from inside the same write that produced
    /// them.
    fn position(&self) -> Option<(u64, u64)>;

    /// §7's checkpoint order, step 1: fsync the appended sample bytes so they
    /// are durable before `segments.json` can claim them.
    ///
    /// Split from [`AudioSource::patch_header`] rather than one combined
    /// `checkpoint` call because the two steps must straddle
    /// `segments.json`'s own atomic write (temp file, `fsync`, `rename(2)`):
    /// fsync every channel's data first, then write `segments.json`, then
    /// patch every channel's header. Patching a header before
    /// `segments.json` is written risks exactly the crash window §7 exists
    /// to prevent — a header declaring frames no `segments.json` on disk yet
    /// accounts for.
    fn fsync_data(&mut self) -> Result<(), Error>;

    /// §7's checkpoint order, step 3: patch the WAV header to declare the
    /// frames [`AudioSource::fsync_data`] just made durable. Call only after
    /// `segments.json` has been written for this checkpoint.
    fn patch_header(&mut self) -> Result<(), Error>;

    /// Contract §6's head-pad: insert `frames` of silence at the start of
    /// this segment on this channel's WAV, so the segment's frame 0 lands on
    /// its shared `start_host_ns` instead of on whichever instant this
    /// channel's hardware happened to come up. For the first segment that is
    /// the start of the file; for a reopened one, where this source began
    /// appending, so no earlier segment moves (TUR-151).
    ///
    /// The orchestrator calls this once per segment, immediately after both
    /// channels' first real buffer has arrived, on whichever channel's frame
    /// 0 was captured later — never on the earlier one, and never more than
    /// once.
    /// Implementations must serialize this against their own worker thread's
    /// concurrent [`AudioSource::position`]-reporting writes (e.g. by taking
    /// the same lock), since unlike the other methods here this one is not
    /// safe to interleave with an in-flight append.
    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), Error>;

    /// Also hand every resampled 16 kHz frame to `tee`, alongside the WAV —
    /// the live-transcription copy TUR-31 settled on (see [`tee`]).
    ///
    /// Call before [`AudioSource::start`]; the tee is picked up when capture
    /// starts. The WAV path must be unchanged whether or not a tee is set, and
    /// [`tee::Tee::offer`] must only ever be called from the source's worker
    /// thread (never the IO callback) and never while holding the writer lock
    /// [`AudioSource::position`] also takes.
    ///
    /// The default ignores it, which is right for a source nobody transcribes
    /// live — the test stubs, chiefly. [`AudioSource::pad_leading_silence`]
    /// implementations should offer the same pad to the tee
    /// ([`tee::Tee::offer_silence`]) so both timelines stay the same length.
    fn tee(&mut self, tee: tee::Tee) {
        let _ = tee;
    }

    /// The sample rates the running source reports and measures, as one
    /// log-ready line, for a caller whose own log needs them (the
    /// permission check's "audio flows but the tone was not recognised"
    /// warning, TUR-84). `None` when not running or not applicable.
    fn rate_report(&self) -> Option<String> {
        None
    }

    /// The device rate this source is resampling from right now, for
    /// `segments.json`'s `mic_device_rate`/`sys_device_rate` (TUR-87).
    /// Informational only. `None` when not running or not known.
    fn device_rate(&self) -> Option<u32> {
        None
    }

    /// Whether the running stream said it died: its device went away, or
    /// the OS invalidated it (TUR-163). The session reads it at every tick
    /// and opens a new segment, the way a default-device change does. A
    /// stream error used to reach only the log, so a microphone removed
    /// under a recording (a MacBook lid closed onto a dock while AirPods
    /// stayed the default input) recorded nothing for the rest of the call.
    fn stream_lost(&self) -> bool {
        false
    }

    /// Whether this source delivers audio without pause while it runs, so
    /// a position that stops moving means the stream stalled (TUR-163). A
    /// loopback with no silence keepalive sends nothing while nothing plays,
    /// and says `false`.
    fn delivers_continuously(&self) -> bool {
        true
    }
}

/// Everything that can go wrong during capture.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The user has not granted microphone or audio-capture permission.
    ///
    /// The UI turns this into the permission-denied onboarding path, so it is a
    /// distinct variant rather than a string.
    #[error("meet-ai does not have permission to record audio")]
    PermissionDenied,

    /// No usable input or output device was found.
    #[error("no audio device available: {0}")]
    NoDevice(String),

    /// Writing the WAV file failed.
    #[error("audio i/o failed")]
    Io(#[from] std::io::Error),

    /// A device exists but its state could not be read.
    #[error("could not read the audio device's state: {0}")]
    DeviceRead(String),

    /// Capture is not implemented for this platform yet.
    #[error("audio capture is not supported on this platform")]
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_map_to_their_spec_filenames_and_labels() {
        assert_eq!(Channel::Mic.wav_filename(), "mic.wav");
        assert_eq!(Channel::System.wav_filename(), "system.wav");
        assert_eq!(Channel::Mic.speaker_label(), "You");
        assert_eq!(Channel::System.speaker_label(), "Others");
    }
}
