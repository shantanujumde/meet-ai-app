//! Pausing and resuming a running recording (TUR-146).
//!
//! A pause stops both channels, exactly as the first half of a segment
//! reopen does ([`super::segment::stop_for`]): the microphone and the tap
//! are closed, so not one sample reaches the WAVs or the live-transcript
//! tees until the resume, and the OS's "microphone in use" light goes out
//! too. The frames so far are written into `segments.json` at once, so a
//! crash mid-pause loses nothing.
//!
//! A resume is the second half of a reopen ([`super::segment::open_next`]):
//! fresh sources on the default devices, appending to the same `mic.wav` and
//! `system.wav`, aligned again, and a new segment with
//! [`reason::RESUMED_AFTER_PAUSE`]. One meeting, one set of files; the pause
//! shows only as the jump in that segment's `start_host_ns`.
//!
//! The app's ticker thread owns the session while it records, so the window
//! cannot call it; it flips a [`PauseSwitch`] instead, and the next
//! [`super::RecordingSession::tick`] (at most [`super::TICK_INTERVAL`] later)
//! carries it out, the same way [`super::SystemDrop`] works. While paused a
//! tick does nothing else: no checkpoint (nothing new to make durable) and no
//! device watch (the resume opens whatever the defaults are by then).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use super::RecordingSession;
use super::segment::{Paths, open_next, stop_for};
use crate::AudioSource;
use crate::segments::reason;

/// A handle for pausing a running session from another thread. Cheap to
/// clone; every clone drives the same session. It holds what the caller
/// wants, not what has happened: the session catches up at its next tick.
#[derive(Debug, Clone, Default)]
pub struct PauseSwitch(Arc<Mutex<bool>>);

impl PauseSwitch {
    /// Ask for the session to be paused (`true`) or recording (`false`).
    pub fn set(&self, paused: bool) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = paused;
    }

    /// Whether a pause is wanted right now.
    pub fn wanted(&self) -> bool {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RecordingSession {
    /// The handle another thread pauses and resumes this session with.
    pub fn pause_switch(&self) -> PauseSwitch {
        self.pause_switch.clone()
    }

    /// Whether both channels are stopped for a pause right now.
    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    /// Stop both channels and hold. A second pause is a no-op. Only the
    /// microphone can fail it, as for a reopen; the session then still holds
    /// its (stopped) sources, so [`RecordingSession::stop`] finishes the files.
    pub(super) fn pause(&mut self) -> Result<(), String> {
        if self.paused.is_some() {
            return Ok(());
        }
        tracing::info!("pausing: stopping both channels");
        let close = stop_for(&mut *self.mic, &mut self.sys, "pause")?;
        self.writer
            .update_frames(close.mic_frames, close.sys_frames);
        self.writer
            .write_atomic(&self.segments_path)
            .map_err(|e| format!("writing segments.json at a pause: {e}"))?;
        self.paused = Some(close);
        Ok(())
    }

    /// Start recording again into the same files, onto the sources
    /// `new_mic` and `new_sys` build. A no-op when not paused. A microphone
    /// that will not restart fails it and leaves the session paused.
    pub(super) fn resume_with(
        &mut self,
        new_mic: impl FnOnce() -> Box<dyn AudioSource>,
        new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
    ) -> Result<(), String> {
        let Some(close) = self.paused else {
            return Ok(());
        };
        // A system-audio denial found while paused (TUR-136): the new segment
        // simply opens without a tap.
        if let Some(why) = self.drop_request.take() {
            tracing::warn!(
                reason = %why,
                "the system track was dropped during the pause; resuming with the microphone only"
            );
            self.want_system = false;
        }
        tracing::info!("resuming: starting both channels again");
        let paths = Paths {
            segments: &self.segments_path,
            mic: &self.mic_path,
            sys: &self.sys_path,
        };
        open_next(
            &mut self.mic,
            &mut self.sys,
            &mut self.writer,
            &paths,
            reason::RESUMED_AFTER_PAUSE,
            &self.tees,
            self.want_system,
            close,
            new_mic,
            new_sys,
        )?;
        self.paused = None;
        self.last_output_device = crate::platform::default_output_device().ok();
        self.last_input_device = crate::platform::default_input_device().ok();
        self.last_checkpoint = Instant::now();
        Ok(())
    }

    /// Catch up with the [`PauseSwitch`]. `Ok(true)` means the session is
    /// paused now, and the rest of the tick has nothing to do.
    pub(super) fn apply_pause_request(
        &mut self,
        new_mic: impl FnOnce() -> Box<dyn AudioSource>,
        new_sys: impl FnOnce() -> Option<Box<dyn AudioSource>>,
    ) -> Result<bool, String> {
        match (self.pause_switch.wanted(), self.is_paused()) {
            (true, false) => self.pause().map(|()| true),
            (false, true) => self.resume_with(new_mic, new_sys).map(|()| false),
            (paused, _) => Ok(paused),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_clone_drives_the_same_switch() {
        let switch = PauseSwitch::default();
        assert!(!switch.wanted(), "a new session records");
        let window = switch.clone();
        window.set(true);
        assert!(switch.wanted());
        window.set(false);
        assert!(!switch.wanted());
    }
}
