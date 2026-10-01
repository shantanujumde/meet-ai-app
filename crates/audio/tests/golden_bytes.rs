//! Golden-output tests for the per-chunk audio path: resampler, WAV writer and
//! tee. The expected hashes were computed from the code *before* the Phase 5
//! allocation clean-up, on fully deterministic synthetic audio, so any change to
//! output bytes (not just to behaviour) fails here.

use std::path::PathBuf;

use audio::tee::tee;
use audio::wav_writer::WavWriter;

/// FNV-1a, 64-bit. No dependency, stable across runs and platforms.
fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Deterministic mixed signal: two tones plus xorshift noise.
fn signal(frames: usize, rate: u32) -> Vec<f32> {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    (0..frames)
        .map(|n| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state % 2001) as f32 / 1000.0 - 1.0;
            let t = n as f32 / rate as f32;
            0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 0.2 * (2.0 * std::f32::consts::PI * 3_100.0 * t).sin()
                + 0.05 * noise
        })
        .collect()
}

fn temp_path(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

/// Chunks shaped like resampler output (~341 frames at 48 kHz, varying), as i16.
fn pipeline_i16(rate: u32, seconds: usize) -> Vec<Vec<i16>> {
    let samples: Vec<i16> = signal(rate as usize / 3 * seconds, rate)
        .into_iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
        .collect();
    let mut chunks = Vec::new();
    let mut at = 0;
    let mut i = 0;
    while at < samples.len() {
        let len = (300 + (i * 37) % 90).min(samples.len() - at);
        chunks.push(samples[at..at + len].to_vec());
        at += len;
        i += 1;
    }
    chunks
}

#[test]
fn golden_wav_file_bytes() {
    let chunks = pipeline_i16(48_000, 3);
    let (_dir, path) = temp_path("golden.wav");
    let mut w = WavWriter::create(&path).unwrap();
    for (i, c) in chunks.iter().enumerate() {
        w.append(c).unwrap();
        if i == chunks.len() / 2 {
            w.fsync_data().unwrap();
            w.patch_header().unwrap();
        }
    }
    w.prepend_silence(1234).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();
    drop(w);

    // Reopen and continue: the cross-segment path.
    let mut w = WavWriter::open_append(&path).unwrap();
    w.append(&[1, -2, 3, i16::MAX, i16::MIN]).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();
    drop(w);

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!((bytes.len(), fnv1a(bytes.iter().copied())), GOLDEN_WAV);
}

#[test]
fn golden_tee_stream() {
    let chunks = pipeline_i16(48_000, 2);
    let (tee, feed) = tee();
    for c in &chunks {
        tee.offer(c);
    }
    tee.offer_silence(7);
    drop(tee);
    let mut bytes = Vec::new();
    while let Ok(c) = feed.try_recv() {
        for s in c {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
    }
    assert_eq!((bytes.len(), fnv1a(bytes.iter().copied())), GOLDEN_TEE);
}

const GOLDEN_WAV: (usize, u64) = (98522, 7810372124852571015);
const GOLDEN_TEE: (usize, u64) = (64014, 4913780262349703497);
