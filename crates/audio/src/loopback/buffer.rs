//! How big a capture buffer to ask WASAPI for.
//!
//! In shared mode the callback period is always the device period (about
//! 10 ms) whatever is asked for; the requested size only sets how much the
//! OS holds for us between callbacks (`cpal` 0.18.2, `wasapi/device.rs`,
//! `build_input_stream_raw_inner`). About 80 ms of headroom means a worker
//! that is late by a few periods loses nothing, without adding latency that
//! matters next to the 200 ms drift gate.

use cpal::{BufferSize, SupportedBufferSize};

/// The headroom asked for, in milliseconds of audio.
pub const TARGET_BUFFER_MS: u64 = 80;

/// Used when the device reports no sample rate.
pub const DEFAULT_BUFFER_FRAMES: u32 = 4096;

/// The smallest buffer asked for, whatever the rate.
pub const MIN_BUFFER_FRAMES: u32 = 256;

/// The largest buffer asked for, whatever the rate.
pub const MAX_BUFFER_FRAMES: u32 = 16384;

// Adapted from github.com/CapSoftware/Cap/crates/scap-cpal/src/lib.rs @ a2a6bd8b1948c48fe92936c265c8402d7fa8ddb3 (MIT)
/// About [`TARGET_BUFFER_MS`] of frames at `sample_rate`, kept inside both
/// our own bounds and the range the device supports. A device that reports
/// no range gets `cpal`'s default.
pub fn safe_buffer_size(supported: &SupportedBufferSize, sample_rate: u32) -> BufferSize {
    match supported {
        SupportedBufferSize::Range { min, max } => {
            let target_frames = if sample_rate > 0 {
                let frames = (u64::from(sample_rate) * TARGET_BUFFER_MS) / 1000;
                frames.clamp(u64::from(MIN_BUFFER_FRAMES), u64::from(MAX_BUFFER_FRAMES)) as u32
            } else {
                DEFAULT_BUFFER_FRAMES
            };
            // `clamp` panics on min > max; a device reporting that gets its max.
            let clamped = if min <= max {
                target_frames.clamp(*min, *max)
            } else {
                *max
            };
            BufferSize::Fixed(clamped)
        }
        SupportedBufferSize::Unknown => BufferSize::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: u32, max: u32) -> SupportedBufferSize {
        SupportedBufferSize::Range { min, max }
    }

    #[test]
    fn asks_for_about_80_ms_at_the_device_rate() {
        assert_eq!(
            safe_buffer_size(&range(1, 1_000_000), 48_000),
            BufferSize::Fixed(3_840)
        );
        assert_eq!(
            safe_buffer_size(&range(1, 1_000_000), 44_100),
            BufferSize::Fixed(3_528)
        );
    }

    #[test]
    fn stays_inside_our_bounds() {
        assert_eq!(
            safe_buffer_size(&range(1, 1_000_000), 1_000),
            BufferSize::Fixed(MIN_BUFFER_FRAMES)
        );
        assert_eq!(
            safe_buffer_size(&range(1, 1_000_000), 768_000),
            BufferSize::Fixed(MAX_BUFFER_FRAMES)
        );
    }

    #[test]
    fn stays_inside_the_device_range() {
        assert_eq!(
            safe_buffer_size(&range(4_096, 8_192), 48_000),
            BufferSize::Fixed(4_096)
        );
        assert_eq!(
            safe_buffer_size(&range(64, 1_024), 48_000),
            BufferSize::Fixed(1_024)
        );
    }

    #[test]
    fn no_rate_or_no_range_falls_back() {
        assert_eq!(
            safe_buffer_size(&range(1, 1_000_000), 0),
            BufferSize::Fixed(DEFAULT_BUFFER_FRAMES)
        );
        assert_eq!(
            safe_buffer_size(&SupportedBufferSize::Unknown, 48_000),
            BufferSize::Default
        );
    }

    #[test]
    fn an_inverted_device_range_does_not_panic() {
        assert_eq!(
            safe_buffer_size(&range(8_192, 1_024), 48_000),
            BufferSize::Fixed(1_024)
        );
    }
}
