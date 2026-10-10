//! Noticing a channel that stopped (TUR-163).
//!
//! A checkpoint used to fail only when a channel had no position at all,
//! which is only true before its first chunk. After that, a stream that
//! stopped delivering (a microphone removed under the recording, a tap whose
//! IO proc stalled) kept answering with its last position, every checkpoint
//! passed, and the recording "succeeded" with an hour of one side and
//! silence for the other.
//!
//! Now every checkpoint also compares each channel's position, the capture
//! time of the next frame it will write, with the host clock: one more than
//! [`STALL_LIMIT_NS`] behind has stopped, and the session opens a new
//! segment on fresh streams, as for a device change
//! (`reason::STREAM_RESTART`). A stream whose error callback said it died
//! ([`AudioSource::stream_lost`]) gets the same, at the next tick.

use super::{RecordingSession, segments};
use crate::segments::Anchor;
use crate::{AudioSource, Channel};

/// How far behind the host clock a channel's position may fall before the
/// channel counts as stopped. A running source latches a new position with
/// every append, every few to few tens of ms; 2 s is far past any of that.
pub(super) const STALL_LIMIT_NS: u64 = 2_000_000_000;

/// A channel found stopped, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stall {
    pub channel: Channel,
    /// How far its position was behind the host clock, in ms.
    pub behind_ms: u64,
}

/// The first channel in `anchor` whose position is more than
/// [`STALL_LIMIT_NS`] behind `now_ns`: the microphone, then the system track
/// when `judge_sys` (there is one, and it delivers without pause). A host
/// time of 0 (no position) is not judged here.
pub(super) fn find(anchor: &Anchor, judge_sys: bool, now_ns: u64) -> Option<Stall> {
    let behind = |host_ns: u64| (host_ns != 0).then(|| now_ns.saturating_sub(host_ns));
    let stalled = |channel, host_ns| {
        behind(host_ns)
            .filter(|&ns| ns > STALL_LIMIT_NS)
            .map(|ns| Stall {
                channel,
                behind_ms: ns / 1_000_000,
            })
    };
    stalled(Channel::Mic, anchor.mic_host_ns).or_else(|| {
        judge_sys
            .then(|| stalled(Channel::System, anchor.sys_host_ns))
            .flatten()
    })
}

/// What the log calls `channel`.
fn name(channel: Channel) -> &'static str {
    match channel {
        Channel::Mic => "microphone",
        Channel::System => "system audio",
    }
}

/// The channel whose stream said it died, if any.
fn lost(mic: &dyn AudioSource, sys: Option<&dyn AudioSource>) -> Option<Channel> {
    if mic.stream_lost() {
        return Some(Channel::Mic);
    }
    sys.filter(|sys| sys.stream_lost()).map(|_| Channel::System)
}

impl RecordingSession {
    /// A stream that said it died gets a new segment (TUR-163). `true`
    /// when one was opened.
    pub(super) fn restart_lost_stream(
        &mut self,
        new_mic: impl FnOnce() -> Box<dyn AudioSource>,
        new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
    ) -> Result<bool, String> {
        let Some(channel) = lost(&*self.mic, self.sys.as_deref()) else {
            return Ok(false);
        };
        tracing::warn!(
            "the {} stream was lost (its device went away or was reset); reopening the segment",
            name(channel)
        );
        self.restart_stream(new_mic, new_sys)?;
        Ok(true)
    }

    /// A channel a checkpoint found stopped gets a new segment (TUR-163).
    pub(super) fn restart_stalled(
        &mut self,
        stall: Stall,
        new_mic: impl FnOnce() -> Box<dyn AudioSource>,
        new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
    ) -> Result<(), String> {
        tracing::warn!(
            "the {} track has written nothing for {} ms; its stream stopped, reopening the \
             segment",
            name(stall.channel),
            stall.behind_ms
        );
        self.restart_stream(new_mic, new_sys)
    }

    /// Close the segment and open the next on fresh streams, the way a
    /// device change does, with `reason::STREAM_RESTART`.
    fn restart_stream(
        &mut self,
        new_mic: impl FnOnce() -> Box<dyn AudioSource>,
        new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
    ) -> Result<(), String> {
        self.reopen_with(segments::reason::STREAM_RESTART, new_mic, new_sys)?;
        self.last_output_device = crate::platform::default_output_device().ok();
        self.last_input_device = crate::platform::default_input_device().ok();
        self.segments_written();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    fn anchor(mic_host_ns: u64, sys_host_ns: u64) -> Anchor {
        Anchor {
            mic_host_ns,
            mic_frames: 1,
            sys_host_ns,
            sys_frames: 1,
        }
    }

    #[test]
    fn channels_that_keep_writing_are_not_stalled() {
        let now = 100 * S;
        assert_eq!(find(&anchor(now - S / 10, now - S), true, now), None);
        // A position a hair ahead of "now" (clock jitter) is fine too.
        assert_eq!(find(&anchor(now + 1, now), true, now), None);
    }

    #[test]
    fn a_microphone_more_than_two_seconds_behind_is_stalled() {
        let now = 100 * S;
        assert_eq!(
            find(&anchor(now - 3 * S, now), true, now),
            Some(Stall {
                channel: Channel::Mic,
                behind_ms: 3_000
            })
        );
    }

    #[test]
    fn a_stalled_system_track_counts_only_when_it_delivers_without_pause() {
        let now = 100 * S;
        let stale_sys = anchor(now, now - 5 * S);
        assert_eq!(
            find(&stale_sys, true, now).map(|s| s.channel),
            Some(Channel::System)
        );
        // A loopback with no keepalive sends nothing while nothing plays.
        assert_eq!(find(&stale_sys, false, now), None);
    }

    #[test]
    fn no_position_is_not_judged() {
        assert_eq!(find(&anchor(0, 0), true, 100 * S), None);
    }
}
