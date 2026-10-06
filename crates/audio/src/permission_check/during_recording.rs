//! The system-audio check, run on a recording that is already going
//! (TUR-136).
//!
//! Record used to wait about 10 s for [`super::check_system`] to start its own
//! tap, play the chime and listen for it before the recording could start.
//! Now the recording starts at once, and this module runs the same closed
//! loop on the recording's own tap: a [`crate::tee::Tee::fan_out`] gives it a
//! second copy of the system stream, so the live transcript's copy is not
//! touched. The verdict rule is unchanged ([`super::verdict`]): only exact
//! zeros after every allowed play was listened to in full is a denial; a
//! listen cut short proves nothing.
//!
//! The chime now plays while the microphone records, so each play holds a
//! [`Hush`] for [`hush_window`]: the live transcript's copies of both
//! channels get silence of the same length for that long, and the chime
//! never reaches `transcript.md`. The WAVs keep it.
//!
//! On an OS where a denied system-audio grant is not silent (or there is no
//! grant at all: Windows, Linux), there is nothing to listen for: the chime
//! plays once as the start sound, hushed the same way, and
//! [`super::check_system`] answers without a tap.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use super::chime_output::ChimeOutput;
use super::{ChannelResult, ChannelState, verdict};
use crate::chime::{self, Clock};
use crate::platform;
use crate::tee::{Hush, TeeFeed};

/// Extra hush past the chime's listen window: the output buffer the play
/// lands in, and the capture pipeline between the tap and the tee.
pub const HUSH_MARGIN_MILLIS: u32 = 200;

/// How long the transcript is hushed for each play: the chime, plus the
/// [`chime::LISTEN_TAIL_MILLIS`] the check already allows for the output's
/// latency to the tap (Bluetooth can take a few hundred ms), plus
/// [`HUSH_MARGIN_MILLIS`]. About 1 s per play.
pub fn hush_window() -> Duration {
    Duration::from_millis(u64::from(
        chime::duration_millis() + chime::LISTEN_TAIL_MILLIS + HUSH_MARGIN_MILLIS,
    ))
}

/// Check system audio on a running recording. `feed` is the check's copy of
/// the recording's system stream; `hush` silences the live transcript's
/// copies while the chime plays. Blocks for up to about
/// [`chime::worst_case_millis`] plus the first-frame wait: run it on its own
/// thread. Never touches the recording itself; the caller acts on the
/// verdict.
pub fn check(feed: TeeFeed, hush: &Hush) -> ChannelResult {
    if !platform::SILENT_SYSTEM_DENIAL {
        hush.hold_for(hush_window());
        platform::start_sound();
        return platform::check_system();
    }
    let output = match ChimeOutput::open() {
        Ok(output) => output,
        Err(detail) => {
            return ChannelResult {
                state: ChannelState::Unmeasurable,
                detail,
            };
        }
    };
    let clock = chime::WallClock::start();
    listen(
        &clock,
        hush,
        || output.play(),
        |left| feed.recv_timeout(left),
    )
}

/// The closed loop minus the output device, so it runs headless with a fake
/// feed and a fake clock: `play` starts one chime, `recv` waits at most its
/// argument for the next chunk of 16 kHz frames. Every play is hushed for
/// [`hush_window`] first.
pub fn listen(
    clock: &impl Clock,
    hush: &Hush,
    mut play: impl FnMut(),
    recv: impl FnMut(Duration) -> Result<Vec<i16>, RecvTimeoutError>,
) -> ChannelResult {
    let hushed_play = || {
        hush.hold_for(hush_window());
        play();
    };
    let listened = chime::listen_live(verdict::RATE, clock, hushed_play, recv);
    // The recording owns the source, so there is no rate report to add.
    verdict::log(&listened, None);
    verdict::system_verdict(&listened)
}

#[cfg(test)]
mod tests;
