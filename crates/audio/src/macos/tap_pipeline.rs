//! The system tap's sample path from raw interleaved IO-proc samples to
//! 16 kHz mono `i16` frames, without Core Audio: downmix, resample, convert.
//!
//! Pulled out of `tap.rs`'s worker thread (TUR-80) so the two things that
//! broke on Bluetooth headsets can be tested without a device: resampling
//! from the rate the IO proc really delivers, and switching that rate
//! mid-stream (A2DP 48 kHz to HFP 16 kHz when a call starts) without the
//! system track's duration drifting from wall time.

use crate::resample::{Resampler, downmix_to_mono};
use crate::segments::SAMPLE_RATE_HZ;

/// More than enough zero chunks to push any sinc tail out on a flush; only a
/// guard against looping forever if the resampler ever produced nothing.
const MAX_FLUSH_CHUNKS: usize = 16;

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// One channel's resampling state, rebuilt when the input rate changes.
pub(crate) struct TapPipeline {
    channels: usize,
    rate: u32,
    resampler: Resampler,
    /// Raw interleaved samples not yet a whole resampler chunk.
    pending: Vec<f32>,
    mono: Vec<f32>,
    resampled: Vec<f32>,
    i16_buf: Vec<i16>,
    /// Mono input frames fed to the current resampler, and the 16 kHz frames
    /// it emitted, so a flush can emit exactly `in * 16000 / rate` in total.
    in_frames: u64,
    out_frames: u64,
}

impl TapPipeline {
    pub(crate) fn new(channels: usize, rate: u32) -> Self {
        let resampler = Resampler::new(rate);
        let channels = channels.max(1);
        let chunk = resampler.input_chunk_frames();
        Self {
            channels,
            rate,
            pending: Vec::with_capacity(chunk * channels + 8192),
            mono: Vec::with_capacity(chunk),
            resampled: Vec::with_capacity(resampler.output_frames_max()),
            i16_buf: Vec::with_capacity(resampler.output_frames_max()),
            resampler,
            in_frames: 0,
            out_frames: 0,
        }
    }

    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }

    fn chunk_raw_len(&self) -> usize {
        self.resampler.input_chunk_frames() * self.channels
    }

    /// Resample `self.mono` (exactly one chunk) and hand at most `limit`
    /// output frames to `sink`.
    fn run_chunk(&mut self, limit: Option<u64>, sink: &mut impl FnMut(&[i16])) {
        self.resampler.process_into(&self.mono, &mut self.resampled);
        let mut take = self.resampled.len();
        if let Some(limit) = limit {
            take = take.min(limit.saturating_sub(self.out_frames) as usize);
        }
        if take == 0 {
            return;
        }
        self.i16_buf.clear();
        self.i16_buf
            .extend(self.resampled[..take].iter().copied().map(f32_to_i16));
        self.out_frames += take as u64;
        sink(&self.i16_buf);
    }

    /// Feed raw interleaved samples; every finished 16 kHz chunk goes to `sink`.
    pub(crate) fn push(&mut self, interleaved: &[f32], sink: &mut impl FnMut(&[i16])) {
        self.pending.extend_from_slice(interleaved);
        let chunk_raw_len = self.chunk_raw_len();
        let mut start = 0;
        while self.pending.len() - start >= chunk_raw_len {
            downmix_to_mono(
                &self.pending[start..start + chunk_raw_len],
                self.channels,
                &mut self.mono,
            );
            start += chunk_raw_len;
            self.in_frames += self.mono.len() as u64;
            self.run_chunk(None, sink);
        }
        self.pending.drain(..start);
    }

    /// Switch to a new input rate at this chunk boundary. The old resampler
    /// is flushed first (its partial chunk and its filter tail), so the
    /// frames it emits in total match the input it was given at its rate
    /// exactly, and the system track keeps pace with the microphone's.
    pub(crate) fn set_rate(&mut self, rate: u32, sink: &mut impl FnMut(&[i16])) {
        if rate == self.rate || rate == 0 {
            return;
        }
        let whole = self.pending.len() / self.channels * self.channels;
        let expected = ((self.in_frames + (whole / self.channels) as u64) as f64
            * SAMPLE_RATE_HZ as f64
            / self.rate as f64)
            .round() as u64;
        let chunk = self.resampler.input_chunk_frames();
        let mut offset = 0;
        for _ in 0..MAX_FLUSH_CHUNKS {
            if self.out_frames >= expected {
                break;
            }
            // The partial chunk first, zero-padded; then pure zeros, which
            // only push the filter's tail out.
            let end = whole.min(offset + chunk * self.channels);
            downmix_to_mono(&self.pending[offset..end], self.channels, &mut self.mono);
            offset = end;
            self.mono.resize(chunk, 0.0);
            self.run_chunk(Some(expected), sink);
        }
        // Keep a trailing partial frame: the ring's next samples complete it.
        self.pending.drain(..whole);

        tracing::info!(
            "system tap resampler switched {} Hz -> {rate} Hz after {} output frames",
            self.rate,
            self.out_frames
        );
        self.resampler = Resampler::new(rate);
        self.rate = rate;
        self.in_frames = 0;
        self.out_frames = 0;
        let max = self.resampler.output_frames_max();
        self.resampled.reserve(max);
        self.i16_buf.reserve(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::tap_rate::effective_input_rate;

    /// Stereo interleaved sine at `rate`, `secs` long.
    fn stereo_tone(rate: u32, secs: f64) -> Vec<f32> {
        let frames = (rate as f64 * secs) as usize;
        let mono = test_support::sine_f32(frames, rate, 440.0, 0.5);
        mono.iter().flat_map(|&s| [s, s]).collect()
    }

    /// Feed `samples` in IO-proc-sized slices (512 stereo frames), like the ring.
    fn feed(p: &mut TapPipeline, samples: &[f32], out: &mut Vec<i16>) {
        for block in samples.chunks(1024) {
            p.push(block, &mut |c: &[i16]| out.extend_from_slice(c));
        }
    }

    #[test]
    fn sixteen_khz_frames_with_a_48_khz_tap_format_keep_their_duration() {
        // TUR-80: the tap reports 48 kHz, the HFP output (and so the IO proc)
        // runs at 16 kHz. Before the fix this produced 1/3 of the frames.
        let rate = effective_input_rate(48_000, Some(16_000.0), Some(16_000.0));
        let mut p = TapPipeline::new(2, rate);
        let input = stereo_tone(16_000, 10.0);
        let mut out = Vec::new();
        feed(&mut p, &input, &mut out);

        let input_frames = input.len() / 2;
        let missing = input_frames - out.len();
        // Only the last partial chunk (< 1024 frames) is still pending.
        assert!(
            missing < 1024,
            "{} of {input_frames} frames out: not 1:1",
            out.len()
        );
    }

    #[test]
    fn the_old_bug_is_what_the_tap_format_alone_produces() {
        // The same 16 kHz input resampled as 48 kHz: the 1/3 the owner saw.
        let mut p = TapPipeline::new(2, 48_000);
        let input = stereo_tone(16_000, 9.0);
        let mut out = Vec::new();
        feed(&mut p, &input, &mut out);
        let ratio = out.len() as f64 / (input.len() / 2) as f64;
        assert!((ratio - 1.0 / 3.0).abs() < 0.01, "ratio {ratio}");
    }

    #[test]
    fn a_rate_change_mid_stream_keeps_the_track_at_wall_time() {
        // Music mode (A2DP 48 kHz) for 3 s, then a call starts (HFP 16 kHz)
        // for 4 s: 7 s of real time must come out as 7 s at 16 kHz.
        let mut p = TapPipeline::new(2, 48_000);
        let mut out = Vec::new();
        feed(&mut p, &stereo_tone(48_000, 3.0), &mut out);
        let mut sink = |c: &[i16]| out.extend_from_slice(c);
        p.set_rate(16_000, &mut sink);
        assert_eq!(p.rate(), 16_000);
        assert_eq!(out.len(), 48_000, "the 48 kHz part flushes to exactly 3 s");

        feed(&mut p, &stereo_tone(16_000, 4.0), &mut out);
        let expected = 16_000 * 7;
        assert!(
            expected - out.len() < 1024,
            "{} frames for 7 s, expected ~{expected}",
            out.len()
        );
    }

    #[test]
    fn switching_back_up_also_flushes_exactly() {
        let mut p = TapPipeline::new(1, 16_000);
        let mut out = Vec::new();
        let tone = test_support::sine_f32(16_000 * 2 + 300, 16_000, 440.0, 0.5);
        feed(&mut p, &tone, &mut out);
        p.set_rate(48_000, &mut |c: &[i16]| out.extend_from_slice(c));
        assert_eq!(out.len(), tone.len(), "16 kHz in, 16 kHz out, 1:1");
    }

    #[test]
    fn the_same_rate_or_zero_is_a_no_op() {
        let mut p = TapPipeline::new(2, 48_000);
        let mut out = Vec::new();
        feed(&mut p, &stereo_tone(48_000, 0.5), &mut out);
        let before = out.len();
        p.set_rate(48_000, &mut |c: &[i16]| out.extend_from_slice(c));
        p.set_rate(0, &mut |c: &[i16]| out.extend_from_slice(c));
        assert_eq!(out.len(), before);
        assert_eq!(p.rate(), 48_000);
    }

    #[test]
    fn a_partial_frame_survives_a_rate_switch() {
        // An odd sample count leaves half a stereo frame pending; it must
        // still pair up with the next sample rather than shift the channels.
        let mut p = TapPipeline::new(2, 48_000);
        let mut out = Vec::new();
        p.push(&[0.1, 0.1, 0.2], &mut |c: &[i16]| out.extend_from_slice(c));
        p.set_rate(16_000, &mut |c: &[i16]| out.extend_from_slice(c));
        assert_eq!(p.pending, vec![0.2]);
    }
}
