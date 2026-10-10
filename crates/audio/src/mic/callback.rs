//! The microphone's capture callback and ring (TUR-163).
//!
//! The microphone used to have a callback per sample format (F32 and I16
//! only, I16 scaled by `i16::MAX`) that pushed what fit into its ring and
//! dropped the rest unseen. It now takes the loopback's path: `cpal`'s raw
//! stream converted by [`cpal_stream::to_f32`] (every format in
//! [`CAPTURE_FORMATS`], one scaling), into a [`Capture`], which pushes whole
//! frames only and turns frames a full ring had no room for into counted
//! silence. [`MicCallback::packet`] is that path without `cpal`'s callback
//! info, so the tests drive it with plain buffers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::{Data, InputCallbackInfo, SampleFormat};

use crate::Error;
use crate::capture_clock::first_frame_ns;
use crate::loopback::capture::Capture;
use crate::loopback::clock::GapRule;
use crate::loopback::cpal_stream::{self, CAPTURE_FORMATS};
use crate::loopback::drain::{self, Drain};
use crate::platform::host_now_ns;
// The OS's capture time, on the host clock: WASAPI's on Windows (TUR-37),
// Core Audio's on macOS (TUR-151).
use crate::platform::input_callback_ns;
use crate::rate_meter::FixedRates;
use crate::tee::Tee;
use crate::track::TrackWriter;

/// What the log calls this channel.
const LABEL: &str = "microphone";

/// Samples the conversion buffer starts with; grown (once) if a packet is
/// ever larger.
const CONVERT_SAMPLES: usize = 16_384;

/// The microphone's ring: [`drain::ring`], with no timestamp ever read as a
/// gap ([`GapRule::NEVER`]), since a microphone delivers without pause and a
/// late callback is jitter, not missing audio.
pub(super) fn ring(
    rates: Arc<FixedRates>,
    channels: usize,
    track: TrackWriter,
    tee: Option<Tee>,
) -> (Capture, Drain<FixedRates>) {
    let (capture, drain) = drain::ring(LABEL, rates, channels, track, tee);
    (capture.with_gap_rule(GapRule::NEVER), drain)
}

/// `Err` for a sample format the callback cannot convert.
pub(super) fn unsupported(format: SampleFormat) -> Result<(), Error> {
    if CAPTURE_FORMATS.contains(&format) {
        return Ok(());
    }
    Err(Error::NoDevice(format!(
        "microphone reports an unsupported sample format: {format:?}"
    )))
}

/// The stream's error callback: a dead stream sets `lost`, which the
/// session reads at its next tick and answers with a new segment, the way
/// the Linux loopback's `note_stream_lost` does (TUR-38).
pub(super) fn on_stream_error(lost: &AtomicBool, error: &cpal::Error) {
    if cpal_stream::stream_is_lost(error.kind()) {
        tracing::warn!("microphone stream lost: {error}; opening a new segment");
        lost.store(true, Ordering::Release);
    } else {
        tracing::warn!("cpal input stream error: {error}");
    }
}

/// The capture callback's state: no allocation once the conversion buffer
/// has grown, no lock, no I/O (SPEC §2.3).
pub(super) struct MicCallback {
    capture: Capture,
    scratch: Vec<f32>,
    channels: usize,
    rate: u32,
}

impl MicCallback {
    pub(super) fn new(capture: Capture, channels: usize, rate: u32) -> Self {
        Self {
            capture,
            scratch: vec![0.0; CONVERT_SAMPLES],
            channels: channels.max(1),
            rate,
        }
    }

    /// One `cpal` callback.
    pub(super) fn on_input(&mut self, data: &Data, info: &InputCallbackInfo) {
        self.packet(data, input_callback_ns(info), host_now_ns());
    }

    /// One packet: `os_ns` is the OS's capture time for it, if any, and
    /// `now_ns` the host clock in the callback. The time is the packet's
    /// first frame's capture, or the callback's own less the packet when the
    /// OS gives none ([`first_frame_ns`]).
    pub(super) fn packet(&mut self, data: &Data, os_ns: Option<u64>, now_ns: u64) {
        let Some(samples) = cpal_stream::to_f32(data, &mut self.scratch) else {
            return;
        };
        let frames = samples.len() / self.channels;
        let first = first_frame_ns(os_ns, now_ns, frames, self.rate);
        self.capture.packet(samples, Some(first), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::drain::ring_sized;
    use crate::rate_meter::Rates;

    const MS: u64 = 1_000_000;
    const RATE: u32 = 48_000;
    /// 10 ms at 48 kHz.
    const PACKET: usize = 480;

    fn data_of<T: cpal::SizedSample>(samples: &mut [T]) -> Data {
        // SAFETY: `samples` outlives the returned `Data` in every test, and
        // its length and format are the slice's own.
        unsafe { Data::from_parts(samples.as_mut_ptr().cast(), samples.len(), T::FORMAT) }
    }

    fn samples_in(path: &std::path::Path) -> Vec<i16> {
        hound::WavReader::open(path)
            .unwrap()
            .samples::<i16>()
            .map(Result::unwrap)
            .collect()
    }

    /// `packets` 10 ms mono packets of `sample` from `make`, pushed before
    /// the worker runs into a ring of `ring` samples (the real size when
    /// `None`); the 16 kHz samples on disk and the frames the ring dropped.
    fn record<T: cpal::SizedSample>(
        packets: u64,
        ring: Option<usize>,
        mut make: impl FnMut(usize) -> T,
    ) -> (Vec<i16>, u64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        let track = TrackWriter::open(&path, LABEL).unwrap();
        let rates = FixedRates::new(LABEL, RATE);
        let (capture, drain) = match ring {
            None => super::ring(Arc::clone(&rates), 1, track.clone(), None),
            Some(samples) => {
                let (capture, drain) =
                    ring_sized(LABEL, Arc::clone(&rates), 1, (track.clone(), None), samples);
                (capture.with_gap_rule(GapRule::NEVER), drain)
            }
        };
        let stats = drain.stats();
        let mut callback = MicCallback::new(capture, 1, rates.effective());
        let now = 10_000 * MS;
        for n in 0..packets {
            let mut packet: Vec<T> = (0..PACKET).map(&mut make).collect();
            let at = now - (packets - n) * 10 * MS;
            callback.packet(&data_of(&mut packet), Some(at), now);
        }
        drain.run(&AtomicBool::new(false), || {});
        track.finish().unwrap();
        (
            samples_in(&path),
            stats.dropped_frames.load(Ordering::Relaxed),
        )
    }

    /// TUR-163's "Done when": an I32 default input records, at the same
    /// scale as the other formats.
    #[test]
    fn an_i32_microphone_records() {
        let (samples, dropped) = record(50, None, |_| i32::MAX / 2);
        assert_eq!(dropped, 0);
        // 0.5 s at 16 kHz, less the resampler's delay and its last chunk.
        assert!(samples.len() > 6_000, "{} frames", samples.len());
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(
            (15_500..=17_000).contains(&peak),
            "a half-scale input reads half scale, got {peak}"
        );
    }

    #[test]
    fn i16_scales_like_the_loopback() {
        // -32768 is full scale, as `to_f32` reads it for the loopback.
        let (samples, _) = record(50, None, |_| i16::MIN / 2);
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!((15_500..=17_000).contains(&peak), "got {peak}");
    }

    /// TUR-163's "Done when": dropped microphone samples become counted
    /// silence, so the track keeps its length.
    #[test]
    fn a_full_microphone_ring_writes_counted_silence() {
        let (whole, _) = record(50, None, |_| 0.25f32);
        let (cut, dropped) = record(50, Some(PACKET * 3), |_| 0.25f32);
        assert_eq!(dropped, 47 * PACKET as u64);
        assert!(
            whole.len().abs_diff(cut.len()) <= 1,
            "{} frames with room, {} with a full ring",
            whole.len(),
            cut.len()
        );
        assert!(cut.iter().any(|&s| s != 0), "what fit is still there");
    }

    #[test]
    fn a_stereo_packet_with_half_a_frame_never_shifts_the_pair() {
        let dir = tempfile::tempdir().unwrap();
        let track = TrackWriter::open(&dir.path().join("mic.wav"), LABEL).unwrap();
        let rates = FixedRates::new(LABEL, RATE);
        // Room for 3 samples: one whole stereo frame, never one and a half.
        let (capture, drain) = ring_sized(LABEL, rates, 2, (track, None), 3);
        let stats = drain.stats();
        let mut callback = MicCallback::new(capture, 2, RATE);
        let mut packet = [0.1f32, 0.2, 0.3, 0.4];
        callback.packet(&data_of(&mut packet), Some(MS), 2 * MS);
        assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn only_a_dead_stream_marks_the_microphone_lost() {
        let lost = AtomicBool::new(false);
        on_stream_error(&lost, &cpal::ErrorKind::Xrun.into());
        assert!(!lost.load(Ordering::Acquire));
        on_stream_error(&lost, &cpal::ErrorKind::DeviceNotAvailable.into());
        assert!(lost.load(Ordering::Acquire));
    }

    #[test]
    fn every_capture_format_is_accepted_and_u8_is_not() {
        for format in CAPTURE_FORMATS {
            assert!(unsupported(format).is_ok(), "{format:?}");
        }
        assert!(matches!(
            unsupported(SampleFormat::U8),
            Err(Error::NoDevice(_))
        ));
    }
}
