//! Closed-loop verification of [`audio::mic::MicSource`] against real
//! hardware: play the permission chime through the default output device,
//! capture it back through the real microphone via the real `AudioSource`
//! pipeline (cpal → resample → `WavWriter`), and confirm the recorded
//! `mic.wav` actually contains it.
//!
//! This is deliberately not a synthetic in-memory test. The point is to
//! prove real signal reached real disk through the real capture path — "it
//! compiled" and "a test that records silence and reports success" are both
//! explicitly the failure modes this guards against. It plays a synthesized
//! tone rather than recording ambient room audio, so nothing captured here is
//! a real conversation.
//!
//! `#[ignore]`d because it needs a real output+input device pair, a
//! microphone TCC grant, and enough room volume for the chime to make the
//! round trip — none of which CI can provide. Run explicitly:
//! `cargo test -p audio --test mic_closed_loop -- --ignored --nocapture`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use audio::chime;
use audio::mic::MicSource;
use audio::{AudioSource, Channel};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[test]
#[ignore = "needs real audio hardware, a mic TCC grant, and audible room volume"]
fn the_chime_played_through_speakers_is_recovered_from_a_real_mic_recording() {
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

    let dir = std::env::temp_dir().join(format!("meet-ai-mic-closed-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("mic.wav");
    let _ = std::fs::remove_file(&dest);

    let mut mic = MicSource::new();
    mic.start(dest.clone()).expect(
        "starting mic capture — if this fails, check System Settings > Privacy > Microphone \
         for the running process, per FINDINGS §10.1's denied-permission finding",
    );
    assert_eq!(mic.channel(), Channel::Mic);

    std::thread::sleep(Duration::from_millis((total_millis + 500) as u64));
    mic.stop().expect("stopping mic capture");
    drop(output_stream);

    let mut reader = hound::WavReader::open(&dest).expect("mic.wav must be a valid, playable WAV");
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
        "mic.wav declared frames but contained none — capture produced an empty file"
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

    // FINDINGS §10.1: a denied capture is bit-exact silence with a noErr-shaped
    // success return, so "the file exists and is nonzero length" proves
    // nothing — only the chime being *found* does.
    let reading = chime::heard(&samples, 16_000);
    for note in &reading.notes {
        eprintln!(
            "  note {:.1}Hz: magnitude {:.5} (other-note {:.5}), passed={}",
            note.expected_hz, note.magnitude, note.other_magnitude, note.passed
        );
    }
    assert!(
        reading.present,
        "the chime was not recovered from the real mic recording — either the mic path is \
         broken, the room is too quiet, or capture is silently denied (see printed magnitudes \
         above and FINDINGS §10.1)"
    );

    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_dir(&dir);
}
