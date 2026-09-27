//! Closed-loop verification of [`audio::macos::tap::SystemSource`] against
//! real hardware: play the permission chime through the default output
//! device, capture it back through the real process-tap pipeline (Core Audio
//! tap → aggregate device → resample → `WavWriter`), and confirm the recorded
//! `system.wav` actually contains it.
//!
//! Unlike [`crate::mic_closed_loop`]'s acoustic round trip through the room,
//! the tap captures the device's own output directly, so no microphone or
//! room volume is involved — only the tap's own TCC grant and Core Audio
//! itself sit between "played" and "captured". That is a *narrower* gap than
//! the mic test's, and it is deliberately a synthesized tone, not a real
//! conversation, so nothing captured here is a real meeting.
//!
//! `#[ignore]`d because it needs a real output device, a system-audio-
//! recording TCC grant, and Core Audio's tap/aggregate-device machinery —
//! none of which CI can provide. Run explicitly:
//! `cargo test -p audio --test system_closed_loop -- --ignored --nocapture`.

#![cfg(target_os = "macos")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use audio::chime;
use audio::macos::tap::SystemSource;
use audio::{AudioSource, Channel};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[test]
#[ignore = "needs a real output device and a system-audio-recording TCC grant"]
fn the_chime_played_through_the_default_output_is_recovered_from_a_real_tap_recording() {
    let host = cpal::default_host();
    let output = host
        .default_output_device()
        .expect("this machine has a default output device");
    let output_config = output
        .default_output_config()
        .expect("default output device has a usable config")
        .config();
    let output_rate = output_config.sample_rate;
    let output_channels = output_config.channels as usize;

    let total_millis = chime::ONSET_TIMEOUT_MILLIS + chime::duration_millis() + 1000;
    let mono = chime::looped_samples(output_rate, total_millis);
    let cursor = Arc::new(AtomicUsize::new(0));

    let dir =
        std::env::temp_dir().join(format!("meet-ai-system-closed-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("system.wav");
    let _ = std::fs::remove_file(&dest);

    // Start the tap *before* playback so the aggregate device is already
    // running when the chime starts — starting it mid-tone would just
    // truncate the onset, not invalidate the test, but this way the full
    // tone is available for chime::heard's search window.
    let mut system = SystemSource::new();
    system.start(dest.clone()).expect(
        "starting system-audio capture — if this fails, check System Settings > Privacy & \
         Security > Screen & System Audio Recording for the running process, per FINDINGS \
         §10.1's denied-permission finding (a denial is bit-exact silence with a noErr-shaped \
         return, not an error here)",
    );
    assert_eq!(system.channel(), Channel::System);

    let play_cursor = Arc::clone(&cursor);
    let output_stream = output
        .build_output_stream(
            output_config,
            move |data: &mut [f32], _| {
                let start = play_cursor.load(Ordering::Relaxed);
                for (i, frame) in data.chunks_mut(output_channels.max(1)).enumerate() {
                    let sample = mono.get(start + i).copied().unwrap_or(0.0);
                    for s in frame {
                        *s = sample;
                    }
                }
                play_cursor.fetch_add(data.len() / output_channels.max(1), Ordering::Relaxed);
            },
            |err| eprintln!("output stream error: {err}"),
            None,
        )
        .expect("building the output stream");
    output_stream.play().expect("starting chime playback");

    std::thread::sleep(Duration::from_millis((total_millis + 500) as u64));
    drop(output_stream);
    system.stop().expect("stopping system-audio capture");

    let mut reader =
        hound::WavReader::open(&dest).expect("system.wav must be a valid, playable WAV");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "capture must land at 16 kHz");
    assert_eq!(spec.channels, 1, "capture must be mono");
    assert_eq!(spec.bits_per_sample, 16, "capture must be s16");

    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    assert!(
        !samples.is_empty(),
        "system.wav declared frames but contained none — capture produced an empty file"
    );

    let rms =
        (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    let zero_fraction = samples.iter().filter(|s| **s == 0.0).count() as f64 / samples.len() as f64;

    eprintln!(
        "captured {} frames ({:.1}s at 16kHz), RMS {rms:.5}, {:.1}% bit-exact-zero samples",
        samples.len(),
        samples.len() as f64 / 16_000.0,
        zero_fraction * 100.0,
    );

    // FINDINGS §10.1: a denied capture is bit-exact silence with a
    // noErr-shaped success return, so "the file exists and is nonzero
    // length" proves nothing — only the chime being *found* does.
    let reading = chime::heard(&samples, 16_000);
    for note in &reading.notes {
        eprintln!(
            "  note {:.1}Hz: magnitude {:.5} (other-note {:.5}), passed={}",
            note.expected_hz, note.magnitude, note.other_magnitude, note.passed
        );
    }
    assert!(
        reading.present,
        "the chime was not recovered from the real system-audio recording — either the tap \
         path is broken or capture is silently denied (see printed magnitudes above and \
         FINDINGS §10.1)"
    );

    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_dir(&dir);
}
