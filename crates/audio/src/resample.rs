//! Device rate → 16 kHz mono (SPEC §2.3), the conversion every capture channel
//! runs before a sample ever reaches [`crate::wav_writer::WavWriter`].
//!
//! One [`Resampler`] per channel, built once at the rate that channel's device
//! negotiated (usually 48 kHz — `FINDINGS.md` §8) and reused for the life of
//! the recording; rebuilding a resampler mid-stream is the expensive path
//! `rubato`'s own docs warn against, and it would also reset the group-delay
//! bookkeeping below.
//!
//! `rubato` 5.0's asynchronous sinc resampler is the intended fit even though
//! the ratio never actually changes here: it matches `Fft`'s quality (per
//! `rubato`'s own README) while giving a documented, queryable
//! [`output_delay`](rubato::Resampler::output_delay) — the exact number this
//! module needs to satisfy the promise already made in the `segments.json`
//! contract (revision 3, §11): *"I subtract the known group delay so anchor
//! readings start near zero... any residual is a constant offset, not a
//! slope."* [`Async::new_sinc`] streamed via
//! [`Resampler::process_into_buffer`] (never `process_all`, which is for a
//! whole clip already in memory) is what leaves that delay in the output in
//! the first place; this module is the one place that trims it back out.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler as _, SincInterpolationParameters};

use crate::segments::SAMPLE_RATE_HZ;

/// Input frames consumed per [`Resampler::process`] call — about 21 ms at
/// 48 kHz. `rubato`'s own docs suggest "a few hundred to a few thousand
/// frames"; this sits in the middle so a checkpoint (§ `CHECKPOINT_INTERVAL_S`,
/// 5 s) is many chunks, never a fraction of one.
const CHUNK_FRAMES: usize = 1024;

/// Resamples one mono channel from its device rate to
/// [`SAMPLE_RATE_HZ`] (16 kHz), trimming the resampler's own startup delay so
/// output frame 0 lines up with input frame 0.
pub struct Resampler {
    inner: Async<f32>,
    /// Output frames still to discard before anything is real signal rather
    /// than the sinc filter's own startup transient. Counted down across
    /// calls because one chunk's output can be shorter than the delay.
    delay_remaining: usize,
    out_buf: Vec<f32>,
}

impl Resampler {
    /// `device_rate` is the input stream's negotiated sample rate. Nothing
    /// here assumes 48 kHz; it just happens to be what SPEC §2.3 expects
    /// `cpal` to hand back.
    ///
    /// # Panics
    ///
    /// If `device_rate` is `0` — not a real device rate, and `rubato` has no
    /// sane ratio to build for it.
    pub fn new(device_rate: u32) -> Self {
        assert!(
            device_rate > 0,
            "device_rate must be a real sample rate, got 0"
        );
        let ratio = SAMPLE_RATE_HZ as f64 / device_rate as f64;
        let params = SincInterpolationParameters::default();
        let inner = Async::<f32>::new_sinc(
            ratio,
            // The ratio never changes mid-recording — a device-rate change
            // tears down and reopens the segment (SPEC §3.4/A5) rather than
            // retuning this resampler — so no adjustment headroom is needed.
            1.0,
            &params,
            CHUNK_FRAMES,
            1,
            FixedAsync::Input,
        )
        .expect("SAMPLE_RATE_HZ / device_rate is always a finite, positive ratio");
        let delay_remaining = inner.output_delay();
        let out_buf = vec![0.0; inner.output_frames_max()];
        Self {
            inner,
            delay_remaining,
            out_buf,
        }
    }

    /// Exactly how many mono input frames one [`Resampler::process`] call
    /// consumes. `rubato`'s own contract: always call this rather than assume
    /// [`CHUNK_FRAMES`], even though this resampler happens to fix input size.
    pub fn input_chunk_frames(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// Resample exactly [`Resampler::input_chunk_frames`] mono input frames.
    ///
    /// Returns the 16 kHz output for this chunk — empty while still inside
    /// the resampler's own startup delay, and never longer than
    /// `output_frames_max()`. Allocates; the per-chunk worker loops call
    /// [`Resampler::process_into`] instead, so this is test-only.
    ///
    /// # Panics
    ///
    /// If `input.len() != self.input_chunk_frames()`.
    #[cfg(test)]
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        self.process_into(input, &mut out);
        out
    }

    /// Same as [`Resampler::process`], but writes the output into `out`
    /// (cleared first), so a caller that keeps `out` across calls allocates
    /// nothing per chunk once its capacity reaches
    /// [`Resampler::output_frames_max`].
    ///
    /// # Panics
    ///
    /// If `input.len() != self.input_chunk_frames()`.
    pub fn process_into(&mut self, input: &[f32], out: &mut Vec<f32>) {
        assert_eq!(
            input.len(),
            self.input_chunk_frames(),
            "must feed exactly one resampler chunk at a time"
        );
        out.clear();
        let in_adapter = InterleavedSlice::new(input, 1, input.len())
            .expect("mono slice matches its own length");
        let out_len = self.out_buf.len();
        let mut out_adapter = InterleavedSlice::new_mut(&mut self.out_buf, 1, out_len)
            .expect("out_buf was sized from output_frames_max at construction");
        let (_read, written) = self
            .inner
            .process_into_buffer(&in_adapter, &mut out_adapter, None)
            .expect("fixed input size always satisfies what the resampler asks for");

        let produced = &self.out_buf[..written];
        if self.delay_remaining >= produced.len() {
            self.delay_remaining -= produced.len();
        } else {
            let skip = std::mem::take(&mut self.delay_remaining);
            out.extend_from_slice(&produced[skip..]);
        }
    }

    /// Capacity a caller's `out` needs so [`Resampler::process_into`] never
    /// grows it: the most frames one chunk can produce.
    pub fn output_frames_max(&self) -> usize {
        self.out_buf.len()
    }
}

/// Average an interleaved multi-channel frame down to mono. SPEC §2.3's
/// `cpal` capture is usually already mono (`FINDINGS.md` §8: built-in and
/// AirPods mics report one input channel), but a device that hands back
/// stereo must not silently drop one side.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    debug_assert!(channels > 0);
    out.clear();
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    out.reserve(interleaved.len() / channels);
    for frame in interleaved.chunks_exact(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, sample_rate: u32, freq_hz: f32) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                let t = n as f32 / sample_rate as f32;
                0.5 * (2.0 * std::f32::consts::PI * freq_hz * t).sin()
            })
            .collect()
    }

    /// Goertzel power at one frequency bin, normalised so a full-scale tone
    /// centred exactly on `freq_hz` reads close to `1.0`. Same technique Tess
    /// used to validate the tone probe (`spikes/phase0a-tcc/tone-probe-margin.py`)
    /// — good enough to prove "this frequency survived", not a spectral
    /// analyser.
    fn goertzel_power(samples: &[f32], sample_rate: u32, freq_hz: f32) -> f32 {
        let n = samples.len() as f32;
        let k = (0.5 + n * freq_hz / sample_rate as f32).floor();
        let omega = 2.0 * std::f32::consts::PI * k / n;
        let coeff = 2.0 * omega.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in samples {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        // Normalise by the same quantity a full-scale amplitude-0.5 sine
        // produces, so the assertions below read as "close to 1.0" rather
        // than some sample-count-dependent magic number.
        power / (n * n * 0.5f32.powi(2) / 4.0)
    }

    fn resample_all(device_rate: u32, input: &[f32]) -> Vec<f32> {
        let mut r = Resampler::new(device_rate);
        let chunk = r.input_chunk_frames();
        let mut out = Vec::new();
        for block in input.chunks(chunk) {
            if block.len() < chunk {
                // Real streams don't hand back a short final chunk mid-flight;
                // drop the remainder rather than pad with silence and skew
                // the frequency check below.
                break;
            }
            out.extend(r.process(block));
        }
        out
    }

    #[test]
    fn frame_count_lands_near_the_expected_ratio() {
        // 5 s of 48 kHz in, ~5 s of 16 kHz out — a 3:1 ratio, exactly what the
        // segments.json contract's anchor formulas assume (§11).
        let input = sine(48_000 * 5, 48_000, 440.0);
        let output = resample_all(48_000, &input);

        let expected = input.len() as f64 / 3.0;
        let actual = output.len() as f64;
        assert!(
            (actual - expected).abs() < expected * 0.01,
            "expected ~{expected} output frames at a 3:1 ratio, got {actual}"
        );
    }

    #[test]
    fn a_440_hz_tone_survives_48k_to_16k_resampling() {
        let input = sine(48_000 * 2, 48_000, 440.0);
        let output = resample_all(48_000, &input);

        // 16 kHz Nyquist is 8 kHz, so 440 Hz is nowhere near the cutoff —
        // this is the easy case, not the interesting one. The interesting
        // one is that it must NOT alias into some other bin.
        let at_tone = goertzel_power(&output, 16_000, 440.0);
        let at_off_bin = goertzel_power(&output, 16_000, 2_000.0);
        assert!(
            at_tone > 0.8,
            "440 Hz power {at_tone} reads as lost, not resampled"
        );
        assert!(
            at_off_bin < 0.05,
            "2000 Hz bin power {at_off_bin} is too high for a pure 440 Hz input — spurious energy"
        );
    }

    #[test]
    fn a_tone_above_the_new_nyquist_does_not_alias_back_into_band() {
        // 12 kHz is well above the 8 kHz Nyquist of a 16 kHz output. A correct
        // anti-aliased resampler removes it before decimating; a naive one
        // would fold it down to |12000 - 16000| = 4000 Hz and it would show
        // up right in the middle of speech band.
        let input = sine(48_000 * 2, 48_000, 12_000.0);
        let output = resample_all(48_000, &input);

        let aliased_bin = goertzel_power(&output, 16_000, 4_000.0);
        assert!(
            aliased_bin < 0.05,
            "power {aliased_bin} at the alias frequency — 12 kHz leaked through instead of being filtered"
        );
    }

    #[test]
    fn a_non_48k_device_rate_still_resamples_correctly() {
        // Not every device is 48 kHz (FINDINGS §8 measured it for the built-in
        // and AirPods mics, but this must not assume it everywhere).
        let input = sine(44_100 * 2, 44_100, 440.0);
        let output = resample_all(44_100, &input);

        let expected = input.len() as f64 * 16_000.0 / 44_100.0;
        assert!(
            (output.len() as f64 - expected).abs() < expected * 0.02,
            "expected ~{expected} frames at 44.1k -> 16k, got {}",
            output.len()
        );

        // A non-integer ratio (44100 -> 16000 is 160/441, not a clean 3:1)
        // costs a little passband flatness near the edge of the analysis
        // window, so this threshold is looser than the 48 kHz case above —
        // the thing being asserted is "clearly present", not "identical
        // level".
        let at_tone = goertzel_power(&output, 16_000, 440.0);
        assert!(
            at_tone > 0.6,
            "440 Hz power {at_tone} reads as lost at a non-48k device rate"
        );
    }

    #[test]
    fn downmix_averages_interleaved_channels() {
        let mut out = Vec::new();
        downmix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2, &mut out);
        assert_eq!(out, vec![0.0, 0.5]);
    }

    #[test]
    fn downmix_is_a_no_op_copy_for_already_mono_input() {
        let mut out = Vec::new();
        downmix_to_mono(&[0.1, 0.2, 0.3], 1, &mut out);
        assert_eq!(out, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn the_startup_group_delay_is_fully_trimmed_not_left_as_leading_near_silence() {
        // Contract §11: "I subtract the known group delay so anchor readings
        // start near zero." Feed a tone from sample zero and confirm the
        // first *returned* samples are already at meaningful tone amplitude,
        // not the sinc filter's own ramp-up.
        let input = sine(48_000, 48_000, 440.0);
        let output = resample_all(48_000, &input);

        assert!(
            !output.is_empty(),
            "delay trimming must not consume the whole tone"
        );
        let first_20ms = &output[..320.min(output.len())];
        let rms = (first_20ms.iter().map(|s| s * s).sum::<f32>() / first_20ms.len() as f32).sqrt();
        assert!(
            rms > 0.1,
            "RMS {rms} over the first 20ms reads as still-trimming delay, not real tone"
        );
    }

    /// FNV-1a over little-endian f32 bits; golden values below were computed
    /// from the pre-Phase-5 implementation.
    fn fnv1a_f32(samples: &[f32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for s in samples {
            for b in s.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    fn noisy_signal(frames: usize, rate: u32) -> Vec<f32> {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        (0..frames)
            .map(|n| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let noise = (state % 2001) as f32 / 1000.0 - 1.0;
                let t = n as f32 / rate as f32;
                0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 3_100.0 * t).sin()
                    + 0.05 * noise
            })
            .collect()
    }

    #[test]
    fn golden_output_bytes_at_48k_and_44k1() {
        for (rate, golden) in [(48_000u32, GOLDEN_48K), (44_100, GOLDEN_44K1)] {
            let out = resample_all(rate, &noisy_signal(rate as usize * 3, rate));
            assert_eq!((out.len(), fnv1a_f32(&out)), golden, "rate {rate}");
        }
    }

    #[test]
    fn golden_downmix_stereo_bytes() {
        let mut out = Vec::new();
        downmix_to_mono(&noisy_signal(2 * 4096, 48_000), 2, &mut out);
        assert_eq!((out.len(), fnv1a_f32(&out)), GOLDEN_DOWNMIX);
    }

    #[test]
    fn process_into_matches_process_and_never_grows_a_presized_buffer() {
        let input = noisy_signal(48_000, 48_000);
        let mut a = Resampler::new(48_000);
        let mut b = Resampler::new(48_000);
        let chunk = a.input_chunk_frames();
        let mut out = Vec::with_capacity(b.output_frames_max());
        let cap = out.capacity();
        for block in input.chunks_exact(chunk) {
            b.process_into(block, &mut out);
            assert_eq!(out, a.process(block));
            assert_eq!(out.capacity(), cap, "steady state must not reallocate");
        }
    }

    const GOLDEN_48K: (usize, u64) = (47744, 18202244382786285904);
    const GOLDEN_44K1: (usize, u64) = (47879, 13416374915378001272);
    const GOLDEN_DOWNMIX: (usize, u64) = (4096, 3792743920406672200);
}
