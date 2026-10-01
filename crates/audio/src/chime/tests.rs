use super::*;

/// The rate the capture path writes (`AudioSource` produces 16 kHz mono),
/// so it is the rate the detector actually runs at.
const CAPTURE_RATE: u32 = 16_000;

fn silence(millis: u32, sample_rate: u32) -> Vec<f32> {
    vec![0.0; samples_in(millis, sample_rate)]
}

#[test]
fn the_decision_is_an_audible_chime_that_survives_the_path() {
    // A6 in prose is an opinion; here it is a failing test if someone
    // quietly moves the tone out of the audible band or out of the band
    // the capture path can carry.
    for note in NOTES {
        assert!(
            (500.0..=3_000.0).contains(&note.hz),
            "{} Hz is outside the band that is both comfortably audible and \
             safe through Bluetooth codecs and the 16 kHz resample",
            note.hz
        );
        assert!(
            note.hz < CAPTURE_RATE as f32 / 2.0,
            "{} Hz is above Nyquist for the captured 16 kHz stream",
            note.hz
        );
        assert!(
            note.hz % 50.0 != 0.0 && note.hz % 60.0 != 0.0,
            "{} Hz collides with a mains harmonic",
            note.hz
        );
    }
    assert!(
        NOTES[1].hz > NOTES[0].hz,
        "the chime rises, so it reads as starting rather than failing"
    );
    assert!(
        (150..=300).contains(&duration_millis()),
        "SPEC §8.1 asks for ~200 ms; got {} ms",
        duration_millis()
    );
}

#[test]
fn the_chime_round_trips_through_synthesis_and_detection() {
    let played = samples(CAPTURE_RATE);
    let reading = heard(&played, CAPTURE_RATE);
    assert!(reading.present, "{reading:?}");
    for note in &reading.notes {
        assert!(
            note.magnitude > 0.5 * AMPLITUDE,
            "an unattenuated chime should read near its amplitude: {note:?}"
        );
    }
}

#[test]
fn the_measured_denial_payload_is_not_heard() {
    // FINDINGS §10.1 and §10.7: on denial every sample is a bit-exact
    // zero, at the normal callback rate, for the full duration. This is
    // that buffer.
    let denied = silence(duration_millis() + 200, CAPTURE_RATE);
    let reading = heard(&denied, CAPTURE_RATE);
    assert!(!reading.present, "{reading:?}");
    for note in &reading.notes {
        assert_eq!(note.magnitude, 0.0, "{note:?}");
    }
}

#[test]
fn a_granted_capture_of_a_quiet_room_is_not_heard() {
    // The case an RMS floor gets wrong: permission is fine, nobody is
    // talking, and there is a little noise on the line. The chime is still
    // absent, so the check must fail — but it must fail because our signal
    // is missing, not because the room was loud enough.
    let mut room = silence(duration_millis() + 200, CAPTURE_RATE);
    for (index, sample) in room.iter_mut().enumerate() {
        *sample = 0.001 * ((index * 7919) % 2000) as f32 / 1000.0 - 0.001;
    }
    let reading = heard(&room, CAPTURE_RATE);
    assert!(!reading.present, "{reading:?}");
}

#[test]
fn a_sustained_source_containing_both_frequencies_is_rejected() {
    // The reason DETECT_CONTRAST exists. A harmonic-rich tone whose
    // partials land on both notes contains each frequency the whole time,
    // so a bare presence test would call it our chime. Our chime plays the
    // notes one after the other, and this does not.
    let len = samples_in(duration_millis() + 100, CAPTURE_RATE);
    let mut both = Vec::with_capacity(len);
    for index in 0..len {
        let seconds = index as f32 / CAPTURE_RATE as f32;
        let first = (std::f32::consts::TAU * NOTES[0].hz * seconds).sin();
        let second = (std::f32::consts::TAU * NOTES[1].hz * seconds).sin();
        both.push(0.4 * (first + second) / 2.0);
    }
    let reading = heard(&both, CAPTURE_RATE);
    assert!(
        !reading.present,
        "both frequencies present throughout is not our chime: {reading:?}"
    );
    // And rejected for the right reason. Each note reads ~0.20 here —
    // forty times DETECT_FLOOR — so a presence-only test would have waved
    // this through. What stops it is that the other note reads just as
    // loud, giving a contrast of ~1.0 against the 4.0 required.
    for note in &reading.notes {
        assert!(
            note.magnitude > 10.0 * DETECT_FLOOR,
            "this must fail on contrast, not on being too quiet: {note:?}"
        );
        assert!(
            note.magnitude < DETECT_CONTRAST * note.other_magnitude,
            "contrast is the test doing the work here: {note:?}"
        );
    }
}

#[test]
fn one_note_on_its_own_is_not_enough() {
    let mut half = samples(CAPTURE_RATE);
    half.truncate(samples_in(NOTES[0].millis, CAPTURE_RATE));
    half.extend(silence(NOTES[1].millis + 100, CAPTURE_RATE));
    let reading = heard(&half, CAPTURE_RATE);
    assert!(!reading.present, "{reading:?}");
    assert!(
        reading.notes[0].passed && !reading.notes[1].passed,
        "the log should show which half arrived: {reading:?}"
    );
}

#[test]
fn device_latency_before_the_chime_does_not_hide_it() {
    // The tap sees our output some tens of milliseconds after we hand it
    // over, and how many is not known in advance. The detector slides.
    for lead_millis in [0, 17, 40, 83] {
        let mut captured = silence(lead_millis, CAPTURE_RATE);
        captured.extend(samples(CAPTURE_RATE));
        captured.extend(silence(120, CAPTURE_RATE));
        let reading = heard(&captured, CAPTURE_RATE);
        assert!(
            reading.present,
            "a {lead_millis} ms lead should not hide the chime: {reading:?}"
        );
    }
}

#[test]
fn the_chime_survives_heavy_attenuation_down_the_path() {
    // A quiet output device is not a permission problem, and calling it
    // one would send a permitted user to a screen whose instructions
    // cannot help them. So the floor has to tolerate real loss: 30 dB of
    // it here, and the check still passes with room to spare.
    let quiet: Vec<f32> = samples(CAPTURE_RATE)
        .iter()
        .map(|sample| sample * 0.0316)
        .collect();
    let reading = heard(&quiet, CAPTURE_RATE);
    assert!(reading.present, "{reading:?}");
    for note in &reading.notes {
        assert!(
            note.magnitude > 1.5 * DETECT_FLOOR,
            "30 dB of loss should not be near the edge of the floor: {note:?}"
        );
    }
}

#[test]
fn the_chime_starts_and_ends_without_a_click() {
    let played = samples(CAPTURE_RATE);
    assert!(played[0].abs() < 1e-6, "fade-in missing: {}", played[0]);
    let last = played[played.len() - 1];
    assert!(last.abs() < 1e-3, "fade-out missing: {last}");
    assert!(
        played.iter().all(|s| s.abs() <= AMPLITUDE + 1e-6),
        "the chime must never exceed its stated amplitude"
    );
}

#[test]
fn a_buffer_too_short_to_hold_the_chime_is_never_a_pass() {
    let stub = samples(CAPTURE_RATE);
    let reading = heard(&stub[..stub.len() / 2], CAPTURE_RATE);
    assert!(!reading.present, "{reading:?}");
}

#[test]
fn the_chime_is_the_same_signal_at_every_device_rate() {
    // Onboarding plays at the output device's rate; the detector reads the
    // captured 16 kHz stream. Both have to agree it is the same chime.
    for rate in [16_000, 44_100, 48_000] {
        let played = samples(rate);
        assert_eq!(played.len(), samples_in(duration_millis(), rate));
        assert!(heard(&played, rate).present, "at {rate} Hz");
    }
}

/// Stands in for the artefact Tess measured at the start of a freshly
/// settled tap: loud, not silence, not our chime. A single tone well
/// clear of both chime notes is enough to exercise the search logic —
/// what matters is that it is not the chime, not that it matches the
/// real artefact's spectrum.
fn settle_window_garbage(millis: u32, sample_rate: u32) -> Vec<f32> {
    (0..samples_in(millis, sample_rate))
        .map(|index| {
            let seconds = index as f32 / sample_rate as f32;
            0.41 * (std::f32::consts::TAU * 300.0 * seconds).sin()
        })
        .collect()
}

#[test]
fn a_single_play_can_be_lost_inside_the_settle_window() {
    // The trap ONSET_TIMEOUT_MILLIS exists to avoid: play the chime once,
    // immediately, and check shortly after — the check window can land
    // entirely inside the tap's unsettled period and never see it.
    let mut captured = settle_window_garbage(1070, CAPTURE_RATE);
    captured.extend(samples(CAPTURE_RATE));
    let checked_shortly_after_starting = &captured[..samples_in(1070 + 50, CAPTURE_RATE)];
    let reading = heard(checked_shortly_after_starting, CAPTURE_RATE);
    assert!(
        !reading.present,
        "a single play landing inside the settle window should be missed: {reading:?}"
    );
}

#[test]
fn looped_playback_survives_the_settle_window() {
    // Same settle window, but the chime plays on repeat across
    // ONSET_TIMEOUT_MILLIS instead of once. At least one repeat lands
    // after the garbage ends, and probe()'s poll finds it.
    let mut captured = settle_window_garbage(1070, CAPTURE_RATE);
    captured.extend(looped_samples(CAPTURE_RATE, ONSET_TIMEOUT_MILLIS));

    let chunk = samples_in(200, CAPTURE_RATE);
    let mut offset = 0;
    let reading = probe(CAPTURE_RATE, || {
        if offset >= captured.len() {
            return None;
        }
        let end = (offset + chunk).min(captured.len());
        let piece = captured[offset..end].to_vec();
        offset = end;
        Some(piece)
    });
    assert!(reading.present, "{reading:?}");
}

#[test]
fn probe_stops_pulling_as_soon_as_the_chime_is_heard() {
    let mut captured = samples(CAPTURE_RATE);
    captured.extend(silence(500, CAPTURE_RATE));
    let chunk = samples_in(50, CAPTURE_RATE);
    let total_chunks = captured.len().div_ceil(chunk);

    let mut pulls = 0;
    let mut offset = 0;
    let reading = probe(CAPTURE_RATE, || {
        pulls += 1;
        if offset >= captured.len() {
            return None;
        }
        let end = (offset + chunk).min(captured.len());
        let piece = captured[offset..end].to_vec();
        offset = end;
        Some(piece)
    });

    assert!(reading.present, "{reading:?}");
    assert!(
        pulls < total_chunks,
        "probe should stop once the chime is confirmed, not drain every pull: \
         pulls={pulls}, available={total_chunks}"
    );
}

#[test]
fn probe_gives_up_after_the_onset_budget_rather_than_pulling_forever() {
    // A denied capture never contains the chime. probe() must still
    // terminate rather than pulling silence forever.
    let chunk = samples_in(100, CAPTURE_RATE);
    let mut pulled = 0usize;
    let reading = probe(CAPTURE_RATE, || {
        pulled += chunk;
        Some(vec![0.0; chunk])
    });

    assert!(!reading.present, "{reading:?}");
    let budget = samples_in(ONSET_TIMEOUT_MILLIS + duration_millis(), CAPTURE_RATE);
    assert!(
        pulled < budget + chunk,
        "probe pulled {pulled} samples, well past its {budget}-sample budget"
    );
}

#[test]
fn looped_samples_covers_the_onset_budget_and_each_period_reads_as_the_chime() {
    let total = looped_samples(CAPTURE_RATE, ONSET_TIMEOUT_MILLIS);
    assert!(
        total.len() >= samples_in(ONSET_TIMEOUT_MILLIS, CAPTURE_RATE),
        "looped playback must cover at least the requested onset budget"
    );

    let period = samples_in(duration_millis() + REPEAT_GAP_MILLIS, CAPTURE_RATE);
    assert!(
        heard(&total[..period], CAPTURE_RATE).present,
        "the first repeat, taken alone, must read as the chime"
    );
}
