//! The tap's IO-proc body: the tap's buffers out of the aggregate's
//! `AudioBufferList`, interleaved, into the shared capture ring (TUR-163).
//!
//! Before TUR-163 the IO proc pushed what fit into its ring and dropped the
//! rest, with no count and no silence put back, and could push half a
//! frame. It now hands each cycle to a [`Capture`], the loopback's: whole
//! frames only, and frames the ring had no room for come back as counted
//! silence (`loopback/drain.rs`). Moved out of `tap.rs` to keep that file
//! under the size limit.

use std::sync::Arc;

use objc2_core_audio_types::AudioBufferList;

use super::tap_buffers::{LayoutProbe, gather_into};
use crate::loopback::capture::Capture;

/// The IO proc's state: the capture, the interleave buffer (sized once, 2 x
/// 16384 frames) and the layout probe. No lock and no allocation once the
/// buffer has grown, so it runs in the IO proc (SPEC §2.3).
pub(super) struct TapCallback {
    capture: Capture,
    interleaved: Vec<f32>,
    probe: Arc<LayoutProbe>,
    channels: usize,
}

impl TapCallback {
    pub(super) fn new(capture: Capture, probe: Arc<LayoutProbe>, channels: usize) -> Self {
        Self {
            capture,
            interleaved: Vec::with_capacity(32_768),
            probe,
            channels,
        }
    }

    /// One IO cycle: `abl` is the cycle's input, captured at `host_ns`.
    ///
    /// # Safety
    /// `abl` must be the `AudioBufferList` Core Audio handed the IO proc for
    /// this cycle: `mNumberBuffers` buffers, each `mData` null or valid for
    /// `mDataByteSize` bytes of `f32`, for the duration of the call.
    pub(super) unsafe fn cycle(&mut self, abl: &AudioBufferList, host_ns: u64) {
        let n = abl.mNumberBuffers as usize;
        if n == 0 {
            return;
        }
        // `mBuffers` is declared `[AudioBuffer; 1]` but is really a C
        // flexible array member — buffer `i` lives at
        // `mBuffers.as_ptr().add(i)`, exactly like the Swift probe's
        // `UnsafeMutableAudioBufferListPointer`.
        let buffers_ptr = abl.mBuffers.as_ptr();
        // SAFETY: `i < mNumberBuffers`, and `mData` is valid for
        // `mDataByteSize` bytes of `f32` per Core Audio's own documented
        // layout for this (non-interleaved) tap format; both outlive this
        // call (the caller's contract). A null `mData` reads as empty.
        let channel = |i: usize| -> &[f32] {
            let buf = unsafe { &*buffers_ptr.add(i) };
            if buf.mData.is_null() {
                return &[];
            }
            let count = buf.mDataByteSize as usize / std::mem::size_of::<f32>();
            unsafe { std::slice::from_raw_parts(buf.mData.cast::<f32>(), count) }
        };
        // SAFETY: as for `channel`, `i < mNumberBuffers`.
        let channels_of = |i: usize| unsafe { (*buffers_ptr.add(i)).mNumberChannels };
        self.probe.observe(n, channels_of);
        let tap = self.probe.layout().range(n);
        let any_frames = tap.clone().map(|i| channel(i).len()).max().unwrap_or(0);
        if any_frames == 0 {
            return;
        }
        let buffer = |i: usize| (channels_of(i), channel(i));
        gather_into(tap, buffer, self.channels, &mut self.interleaved);
        self.capture
            .packet(&mut self.interleaved, Some(host_ns), false);
    }
}
