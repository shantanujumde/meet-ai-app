//! `stub-audio`: capture that touches no device, on any OS (SPEC §8.2).
//!
//! For CI on machines with no microphone, no speakers and no permission to
//! grant. Both sources start and stop cleanly and never produce a sample, and
//! every permission check is [`ChannelState::Unmeasurable`]: nothing was
//! opened, so nothing was measured. Never on in the app build.

use std::path::PathBuf;

use crate::permission_check::{ChannelResult, ChannelState};
use crate::{AudioSource, Channel, Error};

/// An [`AudioSource`] that accepts every call and records nothing.
#[derive(Debug)]
pub(crate) struct NoopSource {
    channel: Channel,
}

impl AudioSource for NoopSource {
    fn start(&mut self, _dest: PathBuf) -> Result<(), Error> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn channel(&self) -> Channel {
        self.channel
    }

    /// No sample was ever written, so there is no position to report.
    fn position(&self) -> Option<(u64, u64)> {
        None
    }

    fn fsync_data(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn pad_leading_silence(&mut self, _frames: u64) -> Result<(), Error> {
        Ok(())
    }
}

fn unmeasurable() -> ChannelResult {
    ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: "audio is stubbed out in this build (the stub-audio feature)".into(),
    }
}

pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    Some(Box::new(NoopSource {
        channel: Channel::System,
    }))
}

pub(crate) fn mic_source() -> Box<dyn AudioSource> {
    Box::new(NoopSource {
        channel: Channel::Mic,
    })
}

pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    None
}

pub(crate) fn check_mic() -> ChannelResult {
    unmeasurable()
}

pub(crate) fn check_system() -> ChannelResult {
    unmeasurable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_sources_start_and_stop_and_produce_nothing() {
        let mut mic = mic_source();
        let mut sys = system_source().expect("the stub always has a system source");
        assert_eq!(mic.channel(), Channel::Mic);
        assert_eq!(sys.channel(), Channel::System);
        for source in [&mut mic, &mut sys] {
            source.start(PathBuf::from("unused.wav")).unwrap();
            source.fsync_data().unwrap();
            source.patch_header().unwrap();
            source.pad_leading_silence(16).unwrap();
            assert_eq!(source.position(), None);
            source.stop().unwrap();
        }
    }

    #[test]
    fn stub_permission_checks_are_unmeasurable() {
        assert_eq!(check_mic().state, ChannelState::Unmeasurable);
        assert_eq!(check_system().state, ChannelState::Unmeasurable);
        assert_eq!(
            crate::permission_check::mic_decision().state,
            ChannelState::Unmeasurable
        );
    }
}
