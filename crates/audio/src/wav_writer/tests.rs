use super::*;

fn tone(frames: usize, freq_hz: f64) -> Vec<i16> {
    test_support::sine_i16(frames, SAMPLE_RATE_HZ, freq_hz, 0.5)
}

fn temp_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

#[test]
fn append_reuses_its_byte_buffer_in_steady_state() {
    let (_dir, path) = temp_path("scratch-capacity.wav");
    let mut w = WavWriter::create(&path).unwrap();
    w.append(&tone(400, 440.0)).unwrap();
    let cap = w.byte_scratch.capacity();
    for len in [341, 342, 400, 17, 399] {
        w.append(&tone(len, 440.0)).unwrap();
        assert_eq!(w.byte_scratch.capacity(), cap);
    }
}

#[test]
fn a_header_only_file_declares_zero_frames_and_is_still_valid_wave() {
    let (_dir, path) = temp_path("empty.wav");
    WavWriter::create(&path).unwrap();

    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 0);
    assert!(samples.is_empty());
}

#[test]
fn a_graceful_stop_writes_back_exactly_what_was_appended_real_signal_not_silence() {
    let (_dir, path) = temp_path("graceful.wav");
    let mut w = WavWriter::create(&path).unwrap();

    let samples = tone(1600, 440.0); // 100 ms of a real 440 Hz tone
    w.append(&samples).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    assert_eq!(w.appended_frames(), 1600);
    assert_eq!(w.header_frames(), 1600);

    let (frames, read_back) = read_declared(&path).unwrap();
    assert_eq!(frames, 1600);
    assert_eq!(read_back, samples, "bytes must round-trip exactly");

    // It is not silence: a real signal has real energy.
    let rms = (read_back.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / read_back.len() as f64)
        .sqrt();
    assert!(rms > 1000.0, "RMS {rms} reads as silence, not a tone");
}

#[test]
fn multiple_checkpoints_accumulate_correctly() {
    let (_dir, path) = temp_path("checkpoints.wav");
    let mut w = WavWriter::create(&path).unwrap();

    for _ in 0..5 {
        let chunk = tone(800, 440.0); // 50 ms per checkpoint
        w.append(&chunk).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();
    }

    assert_eq!(w.header_frames(), 4000);
    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 4000);
    assert_eq!(samples.len(), 4000);
}

/// Reproduces the exact bug found on real hardware (TUR-54, force-quit at
/// 14s / last checkpoint at 10s): the header ended up declaring 342 more
/// frames than `segments.json` had been told about. Root cause was a
/// concurrent worker thread appending more audio in the window between a
/// checkpoint's `fsync_data()` (after which the caller reads `position()`
/// and commits a frame count to `segments.json`) and that same
/// checkpoint's `patch_header()` — which used to re-read the *live*
/// `appended_frames`, ahead of what `segments.json` had just committed to.
/// `sum(*_frames) >= header_frames` must hold in that direction always;
/// this fixture is the other direction and must never reproduce.
#[test]
fn patch_header_never_declares_more_than_the_last_fsync_even_if_more_was_appended_since() {
    let (_dir, path) = temp_path("racing-append.wav");
    let mut w = WavWriter::create(&path).unwrap();

    let checkpoint = tone(80_000, 440.0); // 5 s, a full checkpoint
    w.append(&checkpoint).unwrap();
    w.fsync_data().unwrap();
    // `segments.json` would be written here, committing to 80_000 frames.
    // Simulate the worker thread landing one more chunk in that window,
    // before this checkpoint's patch_header() runs.
    w.append(&tone(1_600, 440.0)).unwrap(); // 100ms, unsynced

    w.patch_header().unwrap();

    assert_eq!(
        w.header_frames(),
        80_000,
        "header must declare exactly what was fsynced and committed to segments.json, \
         not the extra frames a concurrent append landed afterward"
    );
    let (frames, _) = read_declared(&path).unwrap();
    assert_eq!(frames, 80_000);
}

/// The gate condition itself: "whatever was recorded up to that moment is
/// playable, with a valid WAV header" after a `kill -9`. Simulated here by
/// appending audio *past* the last header patch and never patching again
/// — exactly what a killed process leaves behind, since the patch is the
/// last of the three checkpoint steps.
#[test]
fn kill_dash_9_between_checkpoints_leaves_a_valid_playable_prefix() {
    let (_dir, path) = temp_path("killed.wav");
    let mut w = WavWriter::create(&path).unwrap();

    let checkpoint_1 = tone(80_000, 440.0); // 5 s, a full checkpoint
    w.append(&checkpoint_1).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    // The next checkpoint's audio lands on disk, but the process dies
    // before fsync_data/patch_header for it ever run.
    let unpatched = tone(80_000, 440.0);
    w.append(&unpatched).unwrap();
    // No fsync_data(). No patch_header(). This *is* the kill -9.
    drop(w);

    // A conforming reader only trusts the header's declared length.
    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(
        frames, 80_000,
        "header must still declare the last successful checkpoint, not the killed one"
    );
    assert_eq!(samples.len(), 80_000);
    assert_eq!(
        samples, checkpoint_1,
        "the playable prefix must be exact, not truncated audio"
    );

    // The invariant from §7: total appended bytes always exceed or equal
    // what the header admits to, bounded to one checkpoint (80_000
    // frames at 16 kHz / 5 s).
    let actual_len = std::fs::metadata(&path).unwrap().len();
    let actual_frames = (actual_len - HEADER_LEN) / BYTES_PER_SAMPLE as u64;
    assert!(actual_frames >= frames);
    assert!(
        actual_frames - frames <= 80_000,
        "excess must be bounded to one checkpoint, got {} frames",
        actual_frames - frames
    );
}

/// A torn header patch — process dies between the RIFF-size write and the
/// data-size write — must not corrupt the file into an unplayable state.
/// Since `data` size is written second, a torn patch leaves it at its
/// previous (smaller, already-durable) value.
#[test]
fn a_torn_header_patch_leaves_the_data_length_at_its_old_smaller_value() {
    let (_dir, path) = temp_path("torn.wav");
    let mut w = WavWriter::create(&path).unwrap();

    let first = tone(1600, 440.0);
    w.append(&first).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    let second = tone(1600, 440.0);
    w.append(&second).unwrap();
    w.fsync_data().unwrap();

    // Simulate a torn patch: only the RIFF total-size field lands before
    // the crash, not the `data` chunk size.
    w.file.seek(SeekFrom::Start(RIFF_SIZE_OFFSET)).unwrap();
    let new_total = 36 + 2 * (1600 + 1600) as u32;
    w.file.write_all(&new_total.to_le_bytes()).unwrap();
    w.file.sync_all().unwrap();
    drop(w);

    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(
        frames, 1600,
        "data length must still read as the old, fully-durable value"
    );
    assert_eq!(samples, first);
}

#[test]
fn prepend_silence_shifts_real_audio_after_a_silent_head_pad() {
    let (_dir, path) = temp_path("head-pad.wav");
    let mut w = WavWriter::create(&path).unwrap();

    // The channel that came up first already wrote its first buffer
    // before the orchestrator could measure the gap and pad the other
    // channel — exercise that ordering, not the empty-file case.
    let real = tone(160, 440.0); // 10 ms
    w.append(&real).unwrap();

    w.prepend_silence(80).unwrap(); // 5 ms pad
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    assert_eq!(w.header_frames(), 240);
    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 240);
    assert!(
        samples[..80].iter().all(|&s| s == 0),
        "the pad must be silence, not garbage"
    );
    assert_eq!(
        samples[80..],
        real,
        "the real audio must be exact and unshifted past the pad"
    );
}

#[test]
fn prepend_silence_is_a_no_op_for_a_zero_frame_pad() {
    let (_dir, path) = temp_path("no-pad.wav");
    let mut w = WavWriter::create(&path).unwrap();
    let real = tone(160, 440.0);
    w.append(&real).unwrap();

    w.prepend_silence(0).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 160);
    assert_eq!(samples, real);
}

/// A device-change segment reopen: the old segment's writer closes
/// cleanly, a *new* `WavWriter` reopens the same path, and its own
/// checkpoints must land after the first segment's audio, not overwrite
/// or duplicate it — this is what makes "one WAV per channel, segments
/// concatenated in `idx` order" true on disk, not just in `segments.json`.
#[test]
fn open_append_continues_the_same_file_across_a_segment_reopen() {
    let (_dir, path) = temp_path("reopen.wav");

    let first_segment = tone(1600, 440.0); // 100ms
    {
        let mut w = WavWriter::create(&path).unwrap();
        w.append(&first_segment).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();
        assert_eq!(w.header_frames(), 1600);
    }

    let second_segment = tone(800, 880.0); // 50ms, a different tone
    let mut w = WavWriter::open_append(&path).unwrap();
    assert_eq!(
        w.header_frames(),
        1600,
        "reopening must start from the prior segment's declared count, not zero"
    );
    w.append(&second_segment).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    assert_eq!(w.header_frames(), 2400);
    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 2400);
    assert_eq!(&samples[..1600], &first_segment[..], "first segment intact");
    assert_eq!(
        &samples[1600..],
        &second_segment[..],
        "second segment appended immediately after, not overlapping"
    );
}

/// The prior segment's writer was killed before it could patch its
/// header (contract §7's bounded excess): `open_append` must discard the
/// undeclared tail rather than keep it as an unaccounted gap in the
/// middle of the file.
#[test]
fn open_append_truncates_undeclared_bytes_left_by_a_prior_kill_dash_9() {
    let (_dir, path) = temp_path("reopen-after-kill.wav");

    {
        let mut w = WavWriter::create(&path).unwrap();
        w.append(&tone(1600, 440.0)).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap(); // header now declares 1600

        // Simulate a kill -9: more audio lands on disk but is never
        // fsynced or declared.
        w.append(&tone(400, 440.0)).unwrap();
        drop(w);
    }

    let actual_len_before = std::fs::metadata(&path).unwrap().len();
    assert!(
        actual_len_before > HEADER_LEN + 1600 * BYTES_PER_SAMPLE as u64,
        "the undeclared tail must actually be on disk for this test to mean anything"
    );

    let second_segment = tone(400, 880.0);
    let mut w = WavWriter::open_append(&path).unwrap();
    assert_eq!(w.header_frames(), 1600);
    w.append(&second_segment).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();

    assert_eq!(
        w.header_frames(),
        2000,
        "must be exactly the declared 1600 plus the new 400, not the killed tail too"
    );
    let (frames, samples) = read_declared(&path).unwrap();
    assert_eq!(frames, 2000);
    assert_eq!(
        &samples[1600..],
        &second_segment[..],
        "the second segment must follow immediately, with the killed tail discarded"
    );
}

#[test]
fn the_header_is_always_exactly_44_bytes_with_no_extra_chunks() {
    let (_dir, path) = temp_path("shape.wav");
    let mut w = WavWriter::create(&path).unwrap();
    w.append(&tone(160, 440.0)).unwrap();
    w.fsync_data().unwrap();
    w.patch_header().unwrap();
    drop(w);

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        &bytes[36..40],
        b"data",
        "data must be the final chunk header"
    );
    assert_eq!(
        bytes.len() as u64,
        HEADER_LEN + 160 * BYTES_PER_SAMPLE as u64,
        "no chunk may sit between the header and the samples"
    );
}
