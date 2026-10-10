//! Which of the aggregate device's IO buffers are the tap's (TUR-87).
//!
//! The aggregate carries the default output device as a sub-device, and the
//! tap is added after it. When that output device has input streams of its
//! own (a Bluetooth headset in HFP, a USB headset), the IO proc's
//! `AudioBufferList` carries that device's microphone first and the tap's
//! stream last. Interleaving every buffer mixed the headset mic into
//! `system.wav`, and when the layouts differed (a mono mic buffer next to an
//! interleaved stereo tap buffer) the downmix paired the wrong samples.
//!
//! So the tap's buffers are picked by position: the tap is the aggregate's
//! last input stream (`kAudioDevicePropertyStreams`, input scope), and its
//! buffers are the last ones in the list. Each buffer's `mNumberChannels` is
//! honoured when interleaving. [`gather_into`] is pure, so it is tested with
//! synthetic buffer lists; [`LayoutProbe`] records what the first callback
//! saw, for one log line a device check can read.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use objc2_core_audio::{self as ca, AudioObjectID};
use objc2_core_audio_types::{AudioStreamBasicDescription, kAudioFormatFlagIsNonInterleaved};

use super::props::read;
use super::tap_rate::input_streams;

/// How many buffers at the end of the IO proc's list belong to the tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TapBuffers {
    count: usize,
}

impl TapBuffers {
    /// The buffers one stream of this format occupies: one per channel when
    /// it is non-interleaved, otherwise one interleaved buffer.
    pub(crate) fn for_format(channels: u32, flags: u32) -> Self {
        let count = if flags & kAudioFormatFlagIsNonInterleaved != 0 {
            channels.max(1) as usize
        } else {
            1
        };
        Self { count }
    }

    /// The tap's buffers in a list of `n`: the last [`Self::for_format`] ones,
    /// or all of them if the list is shorter.
    pub(crate) fn range(self, n: usize) -> Range<usize> {
        n.saturating_sub(self.count)..n
    }

    /// Read the aggregate's input streams and size the tap's share from the
    /// last one's virtual format, falling back to the tap's own format.
    pub(crate) fn read(
        aggregate_id: AudioObjectID,
        tap_format: &AudioStreamBasicDescription,
    ) -> Self {
        let fallback = Self::for_format(tap_format.mChannelsPerFrame, tap_format.mFormatFlags);
        let streams = input_streams(aggregate_id).unwrap_or_default();
        // SAFETY: `kAudioStreamPropertyVirtualFormat` is an `AudioStreamBasicDescription`.
        let last: Option<AudioStreamBasicDescription> = streams.last().and_then(|&stream| unsafe {
            read(
                stream,
                ca::kAudioStreamPropertyVirtualFormat,
                ca::kAudioObjectPropertyScopeGlobal,
            )
            .ok()
        });
        let chosen = last
            .filter(|format| format.mChannelsPerFrame > 0)
            .map_or(fallback, |format| {
                Self::for_format(format.mChannelsPerFrame, format.mFormatFlags)
            });
        tracing::info!(
            "system tap buffers: aggregate has {} input stream(s); the tap is the last, {} \
             buffer(s) (tap format {} ch, flags 0x{:x})",
            streams.len(),
            chosen.count,
            tap_format.mChannelsPerFrame,
            tap_format.mFormatFlags,
        );
        chosen
    }
}

/// Interleave the buffers in `selected` into `out`, exactly `out_channels`
/// samples per frame. `buffer(i)` gives buffer `i`'s `mNumberChannels` (0
/// reads as 1) and its samples, interleaved within the buffer.
///
/// Frames are the fewest any selected buffer holds. Channels are taken in
/// buffer order; past `out_channels` they are dropped, and if there are
/// fewer, the last one is repeated, so the downmix after this always sees
/// whole frames of the width it expects.
///
/// No allocation once `out` has grown to its steady-state size, so it runs
/// in the IO proc (SPEC §2.3).
pub(crate) fn gather_into<'a>(
    selected: Range<usize>,
    buffer: impl Fn(usize) -> (u32, &'a [f32]),
    out_channels: usize,
    out: &mut Vec<f32>,
) {
    out.clear();
    let width = out_channels.max(1);
    let frames = selected
        .clone()
        .map(|i| {
            let (channels, data) = buffer(i);
            data.len() / channels.max(1) as usize
        })
        .min()
        .unwrap_or(0);
    if frames == 0 {
        return;
    }
    out.resize(frames * width, 0.0);
    let mut column = 0;
    for i in selected {
        let (channels, data) = buffer(i);
        let channels = channels.max(1) as usize;
        for c in 0..channels.min(width - column) {
            for f in 0..frames {
                out[f * width + column] = data[f * channels + c];
            }
            column += 1;
        }
        if column == width {
            break;
        }
    }
    if column > 0 {
        for c in column..width {
            for f in 0..frames {
                out[f * width + c] = out[f * width + column - 1];
            }
        }
    }
}

/// The most buffers whose channel counts [`LayoutProbe`] keeps (one byte each).
const PROBED_BUFFERS: usize = 8;

/// What the first IO callback's buffer list looked like: written once from
/// the IO proc with atomics only, read once by the worker for the log.
#[derive(Debug)]
pub(crate) struct LayoutProbe {
    /// Which buffers the IO proc reads as the tap.
    layout: TapBuffers,
    seen: AtomicBool,
    buffers: AtomicU32,
    /// Up to [`PROBED_BUFFERS`] `mNumberChannels`, one byte each (capped at 255).
    channels: AtomicU64,
    logged: AtomicBool,
}

impl LayoutProbe {
    pub(crate) fn new(layout: TapBuffers) -> Self {
        Self {
            layout,
            seen: AtomicBool::new(false),
            buffers: AtomicU32::new(0),
            channels: AtomicU64::new(0),
            logged: AtomicBool::new(false),
        }
    }

    /// Which buffers are the tap's.
    pub(crate) fn layout(&self) -> TapBuffers {
        self.layout
    }

    /// Record a callback's buffer list, the first time only.
    pub(crate) fn observe(&self, buffers: usize, channels: impl Fn(usize) -> u32) {
        if self.seen.load(Ordering::Relaxed) {
            return;
        }
        let mut packed = 0u64;
        for i in 0..buffers.min(PROBED_BUFFERS) {
            packed |= u64::from(channels(i).min(255)) << (8 * i);
        }
        self.channels.store(packed, Ordering::Relaxed);
        self.buffers.store(buffers as u32, Ordering::Relaxed);
        self.seen.store(true, Ordering::Release);
    }

    /// The log line, once, after the first callback was recorded.
    pub(crate) fn take_report(&self) -> Option<String> {
        if !self.seen.load(Ordering::Acquire) || self.logged.swap(true, Ordering::AcqRel) {
            return None;
        }
        let n = self.buffers.load(Ordering::Relaxed) as usize;
        let packed = self.channels.load(Ordering::Relaxed);
        let channels: Vec<u64> = (0..n.min(PROBED_BUFFERS))
            .map(|i| (packed >> (8 * i)) & 0xff)
            .collect();
        let range = self.layout.range(n);
        Some(format!(
            "system tap IO buffers: mNumberBuffers {n}, mNumberChannels per buffer {channels:?}; \
             reading buffers {}..{} as the tap",
            range.start, range.end
        ))
    }
}

#[cfg(test)]
mod tests;
