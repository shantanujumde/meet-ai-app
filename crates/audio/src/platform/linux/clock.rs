//! The Linux host clock (TUR-38): CLOCK_MONOTONIC, the clock PipeWire stamps
//! its graph cycles with (`pw_stream_get_time_n`), so [`host_now_ns`], the
//! microphone's [`input_callback_ns`] and the loopback's packet times are one
//! domain and the two tracks' anchors can be compared (contract §11).

use cpal::InputCallbackInfo;

use crate::loopback::clock::capture_on_host_clock;

// Adapted from github.com/RustAudio/cpal/src/host/pipewire/stream.rs @ e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7 (Apache-2.0)
/// CLOCK_MONOTONIC now, in ns. 0 if it cannot be read, which Linux documents
/// only for a bad clock id or pointer.
pub(crate) fn host_now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime only writes the timespec it is handed.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc != 0 {
        return 0;
    }
    timespec_ns(ts.tv_sec, ts.tv_nsec)
}

fn timespec_ns(secs: libc::time_t, nanos: libc::c_long) -> u64 {
    let secs = u64::try_from(secs).unwrap_or(0);
    let nanos = u64::try_from(nanos).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

fn instant_ns(instant: cpal::StreamInstant) -> u64 {
    u64::try_from(instant.as_nanos()).unwrap_or(u64::MAX)
}

/// When the callback's first frame was captured, on [`host_now_ns`]'s
/// clock. `cpal`'s PipeWire host already is on it; its PulseAudio and ALSA
/// hosts count from the stream's start, so only their latency is kept
/// ([`capture_on_host_clock`]).
pub(crate) fn input_callback_ns(info: &InputCallbackInfo) -> Option<u64> {
    let timestamp = info.timestamp();
    capture_on_host_clock(
        instant_ns(timestamp.callback),
        instant_ns(timestamp.capture),
        host_now_ns(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_clock_moves_forward() {
        let a = host_now_ns();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = host_now_ns();
        assert!(a > 0, "CLOCK_MONOTONIC read 0");
        assert!(b >= a + 4_000_000, "{a} then {b}");
    }

    #[test]
    fn a_timespec_becomes_ns() {
        assert_eq!(timespec_ns(2, 500), 2_000_000_500);
        assert_eq!(timespec_ns(-1, 5), 5);
    }
}
