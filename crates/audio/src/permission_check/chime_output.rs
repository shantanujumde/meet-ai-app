//! The check chime's output stream, shared by both system-audio checks: the
//! onboarding one with its own tap ([`super::check_system_with`]) and the one
//! that runs during a recording ([`super::during_recording`], TUR-136).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::chime;

/// A running output stream on the default output device that plays the
/// chime once each time [`ChimeOutput::play`] asks, and silence otherwise.
/// Dropping it stops the stream.
pub(super) struct ChimeOutput {
    _stream: cpal::Stream,
    restart: Arc<AtomicBool>,
}

impl ChimeOutput {
    /// Open and start the stream. `Err` is the sentence the check reports
    /// as unmeasurable: no output device, no usable config, or a stream
    /// that would not build or start.
    pub(super) fn open() -> Result<Self, String> {
        let host = cpal::default_host();
        let output = host.default_output_device().ok_or_else(|| {
            "this computer has no default output device to play the check tone through".to_string()
        })?;
        let config = output
            .default_output_config()
            .map_err(|error| format!("the default output device has no usable config: {error}"))?
            .config();
        let channels = (config.channels as usize).max(1);

        // The chime, rendered once at the output device's rate. The output
        // callback plays it from the top each time `play` asks, then falls
        // silent until asked again: one chime per play, never a loop.
        let mono = chime::samples(config.sample_rate);
        let restart = Arc::new(AtomicBool::new(false));
        let callback_restart = Arc::clone(&restart);
        // Start past the end: silence until the first play request.
        let mut position = mono.len();

        let stream = output
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    // `swap` reads and clears the request in one step, so a
                    // play asked for mid-buffer starts on the next buffer,
                    // exactly once.
                    if callback_restart.swap(false, Ordering::AcqRel) {
                        position = 0;
                    }
                    let frames = data.len() / channels;
                    for (i, frame) in data.chunks_mut(channels).enumerate() {
                        let sample = mono.get(position + i).copied().unwrap_or(0.0);
                        for s in frame {
                            *s = sample;
                        }
                    }
                    position = position.saturating_add(frames);
                },
                |error| tracing::warn!(%error, "permission-check output stream error"),
                None,
            )
            .map_err(|error| format!("could not build the check-tone output stream: {error}"))?;
        stream
            .play()
            .map_err(|error| format!("could not start playback of the check tone: {error}"))?;
        Ok(Self {
            _stream: stream,
            restart,
        })
    }

    /// Play the chime once from the top, starting on the next buffer.
    /// Returns at once.
    pub(super) fn play(&self) {
        self.restart.store(true, Ordering::Release);
    }
}
