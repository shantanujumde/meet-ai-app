//! The start sound off macOS (TUR-51).
//!
//! On macOS the chime is the system-audio check's positive control, so it
//! already sounds at the start of every recording. Elsewhere there is no such
//! check, so the same chime is played once, plainly, through the default
//! output device: the user still hears that recording has begun.

use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::chime;

/// How long to keep the stream open past the chime, so the device's buffer
/// drains before the stream is dropped.
const TAIL_MILLIS: u64 = 150;

/// Play the chime once and return when it has finished. An error names why
/// it could not play; a missing start sound never stops a recording.
pub fn play() -> Result<(), String> {
    let host = cpal::default_host();
    let output = host
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let config = output
        .default_output_config()
        .map_err(|error| format!("the default output device has no usable config: {error}"))?
        .config();
    let channels = (config.channels as usize).max(1);
    let mono = chime::samples(config.sample_rate);
    let mut position = 0usize;

    let stream = output
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(channels) {
                    let sample = mono.get(position).copied().unwrap_or(0.0);
                    frame.fill(sample);
                    position = position.saturating_add(1);
                }
            },
            |error| tracing::warn!(%error, "start-sound output stream error"),
            None,
        )
        .map_err(|error| format!("could not build the start-sound stream: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("could not play the start sound: {error}"))?;
    std::thread::sleep(Duration::from_millis(
        u64::from(chime::duration_millis()) + TAIL_MILLIS,
    ));
    Ok(())
}
