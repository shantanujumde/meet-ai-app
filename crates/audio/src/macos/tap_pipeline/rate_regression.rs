//! TUR-84's regression: the IO proc delivers 16 kHz or 44.1 kHz frames while
//! the aggregate's input stream (and, in the worst case, every rate Core
//! Audio reports) claims 48 kHz. The real IO-proc meter ([`CallbackMeter`])
//! and the real worker step ([`TapPipeline::follow`]) run on a fake device
//! clock; the system track must come out at wall time 1:1, with the chime
//! still recognisable.

use std::sync::Arc;

use super::*;
use crate::chime;
use crate::macos::tap_rate::RateState;
use crate::macos::tap_rate::{CallbackMeter, ReportedRates};
use crate::segments::SAMPLE_RATE_HZ;

/// Frames per IO callback.
const BLOCK: usize = 512;
const SECS: f64 = 3.0;

/// What the owner's buds reported: the stream says 48 kHz, the aggregate and
/// the output run at the real rate.
fn owner_reports(rate: u32) -> ReportedRates {
    ReportedRates {
        tap_format: 48_000,
        aggregate_nominal: Some(f64::from(rate)),
        output_nominal: Some(f64::from(rate)),
        stream_virtual: Some(48_000.0),
    }
}

/// Every reported rate lies: only the measurement can tell.
fn all_claim_48k() -> ReportedRates {
    ReportedRates {
        tap_format: 48_000,
        aggregate_nominal: Some(48_000.0),
        output_nominal: Some(48_000.0),
        stream_virtual: Some(48_000.0),
    }
}

/// [`SECS`] of stereo audio at `rate`: silence, then the chime at 1.2 s
/// (where the permission check plays it), then silence.
fn device_audio(rate: u32) -> Vec<f32> {
    let total = (f64::from(rate) * SECS) as usize;
    let mut mono = vec![0.0f32; total];
    let at = (f64::from(rate) * 1.2) as usize;
    for (i, s) in chime::samples(rate).into_iter().enumerate() {
        mono[at + i] = s;
    }
    mono.iter().flat_map(|&s| [s, s]).collect()
}

/// Run the IO proc and the worker over the device's audio, one callback at
/// a time, with host times from the device's true `rate`.
fn record(rate: u32, reported: ReportedRates) -> Vec<i16> {
    let rates = RateState::fake(reported);
    let mut meter = CallbackMeter::new(Arc::clone(&rates), 2);
    let mut pipeline = TapPipeline::new("system tap", 2, rates.effective());
    let mut out = Vec::new();
    for (n, block) in device_audio(rate).chunks(BLOCK * 2).enumerate() {
        let host_ns = (n * BLOCK) as f64 * 1e9 / f64::from(rate);
        meter.observe(host_ns as u64 + 1_000, block.len());
        let mut sink = |c: &[i16]| out.extend_from_slice(c);
        pipeline.follow(&*rates, &mut sink);
        pipeline.push(block, &mut sink);
    }
    // Switching rate flushes every frame given so far (TUR-80's exact flush).
    pipeline.set_rate(8_000, &mut |c: &[i16]| out.extend_from_slice(c));
    out
}

fn assert_wall_time_and_chime(rate: u32, reported: ReportedRates, label: &str) {
    let out = record(rate, reported);
    let wall = (SAMPLE_RATE_HZ as f64 * SECS) as usize;
    assert!(
        out.len().abs_diff(wall) <= 2,
        "{label} at {rate} Hz: {} frames for {SECS} s, expected ~{wall}",
        out.len()
    );
    let captured: Vec<f32> = out
        .iter()
        .map(|&s| f32::from(s) / f32::from(i16::MAX))
        .collect();
    let reading = chime::heard(&captured, SAMPLE_RATE_HZ);
    assert!(reading.present, "{label} at {rate} Hz: {reading:?}");
}

#[test]
fn frames_at_16_and_44_1_khz_under_a_48_khz_stream_keep_wall_time_and_the_chime() {
    for rate in [16_000, 44_100] {
        assert_wall_time_and_chime(rate, owner_reports(rate), "owner's reports");
        assert_wall_time_and_chime(rate, all_claim_48k(), "all reports 48 kHz");
    }
}

#[test]
fn built_in_speakers_at_48_khz_are_unchanged() {
    assert_wall_time_and_chime(48_000, all_claim_48k(), "built-in speakers");
}

#[test]
fn without_the_fix_the_chime_is_lost_at_16_khz() {
    // The pre-TUR-84 choice: the stream format, 48 kHz, for 16 kHz frames.
    let mut pipeline = TapPipeline::new("system tap", 2, 48_000);
    let mut out = Vec::new();
    for block in device_audio(16_000).chunks(BLOCK * 2) {
        pipeline.push(block, &mut |c: &[i16]| out.extend_from_slice(c));
    }
    let ratio = out.len() as f64 / (SAMPLE_RATE_HZ as f64 * SECS);
    assert!((ratio - 1.0 / 3.0).abs() < 0.02, "ratio {ratio}");
}

#[test]
fn reports_lower_than_the_delivered_rate_still_keep_wall_time() {
    // The other direction: everything claims 16 kHz, frames come at 48 kHz.
    // The first window was emitted 3x too long, so the excess is dropped.
    let low = ReportedRates {
        tap_format: 16_000,
        aggregate_nominal: Some(16_000.0),
        output_nominal: Some(16_000.0),
        stream_virtual: Some(16_000.0),
    };
    let out = record(48_000, low);
    let wall = (SAMPLE_RATE_HZ as f64 * SECS) as usize;
    assert!(
        out.len().abs_diff(wall) <= 2,
        "{} frames, expected {wall}",
        out.len()
    );
}
