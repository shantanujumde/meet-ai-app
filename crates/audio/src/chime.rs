//! The recording-start chime — and the proof that it came back.
//!
//! # Why a chime exists at all
//!
//! meet-ai cannot ask macOS whether audio-capture permission was granted. On
//! denial every `OSStatus` is `noErr`, the tap is created, the device starts,
//! and the callbacks fire at the normal rate delivering bit-exact zeros
//! (FINDINGS §10.1, confirmed against a real **Don't Allow** in §10.7). A
//! *granted* capture of a silent Mac is byte-for-byte identical. So a return
//! code is not evidence, and an RMS floor is not evidence either — it would
//! call "nobody has started talking yet" a permission failure.
//!
//! SPEC §8.1 therefore requires a **positive control**: play a short known
//! signal from meet-ai's own process, and look for it coming back through the
//! tap. The process tap is global, so our own output is inside it. Finding the
//! signal proves permission *and* proves the whole tap → WAV path, which the
//! return codes never proved.
//!
//! # The decision this module encodes (SPEC amendment A6)
//!
//! The signal is an **audible** two-note chime, played at onboarding **and at
//! the start of every recording**. Audible, because the control has to survive
//! the exact path it is testing and quiet or ultrasonic content is the first
//! thing that path throws away — and because the app wants a "recording
//! started" cue anyway, so the check is free in UX terms. Every recording,
//! because macOS lets someone revoke the grant in System Settings at any time
//! and tells the app nothing; an onboarding-only check would let meet-ai
//! record an hour of silence and report success.
//!
//! # Why these two notes
//!
//! - **A5 (880 Hz) then E6 (1318.5 Hz)**, a rising perfect fifth. Both sit in
//!   the 500 Hz – 3 kHz band that every link in the chain passes intact:
//!   Bluetooth codecs (AAC/SBC roll off far above this), 44.1 ↔ 48 kHz device
//!   resampling, and the capture path's own resample to 16 kHz mono, whose
//!   Nyquist is 8 kHz. Rising reads as "starting" rather than "error".
//! - **Neither is a multiple of 50 or 60 Hz**, so mains hum and its harmonics
//!   cannot climb into either bin.
//! - **Two notes in sequence, not one.** A single tone can be faked by
//!   coincidence — music, a voice, a notification. Requiring the first note to
//!   dominate in the first window and the second to dominate in the second
//!   makes an accidental pass very unlikely. It also rejects a *sustained*
//!   harmonic-rich source that happens to contain both frequencies at once,
//!   which a bare presence test would wave through: see
//!   [`DETECT_CONTRAST`].
//!
//! # ⚠️ One thing that is not measured yet
//!
//! Whether the chime still reaches the tap when the user's output is **muted
//! or at zero volume**. If it does not, a muted Mac would look exactly like a
//! denial and we would send a permitted user to the denial screen. That is a
//! capture-layer measurement, tracked separately; until it lands, treat "no
//! chime" as *needs explaining*, never as a bare "permission denied".

/// One note of the chime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Frequency in hertz.
    pub hz: f32,
    /// How long it sounds, including its fades.
    pub millis: u32,
}

/// The chime, in order. A5 up to E6.
pub const NOTES: [Note; 2] = [
    Note {
        hz: 880.0,
        millis: 110,
    },
    Note {
        hz: 1318.51,
        millis: 110,
    },
];

/// Peak amplitude, full-scale. −12 dBFS: a normal UI-sound level, and far
/// enough below clipping that a device with its own gain cannot square it off.
///
/// FINDINGS §8/§10 pushed amplitude 0.5 through this path and read peak 0.93
/// back, so the path is roughly unity. Half that stays comfortable to listen
/// to and still leaves 34 dB of headroom over [`DETECT_FLOOR`].
pub const AMPLITUDE: f32 = 0.25;

/// Raised-cosine fade in and out on each note. Without it the note's hard edge
/// clicks, which is both unpleasant and smears energy across every bin —
/// including the bin belonging to the *other* note, which would weaken the
/// contrast test.
pub const FADE_MILLIS: u32 = 8;

/// The quietest a note may read and still count as heard, as a fraction of
/// full scale. −46 dBFS.
///
/// Set deliberately low, because the two error directions are not symmetric:
///
/// - A **false pass** on a denial is impossible at any floor. The denied path
///   is bit-exact zeros (FINDINGS §10.1, §10.7), so the reading is exactly
///   `0.0` — not a small number, zero. Lowering the floor costs nothing here.
/// - A **false failure** sends a user who *does* have permission to the
///   denial screen, where the instructions cannot help them, because nothing
///   is wrong with their permissions. That is the expensive mistake, and a
///   generous floor is what avoids it.
///
/// So this sits 34 dB below the amplitude we play: the chime can lose most of
/// its level to a quiet output device and still be believed. The work of *not*
/// mistaking something else for the chime is done by [`DETECT_CONTRAST`], not
/// by this number.
pub const DETECT_FLOOR: f32 = 0.005;

/// How much louder the expected note must be than the *other* note, measured
/// in the same window.
///
/// This is the test that a bare presence check fails. A sustained
/// harmonic-rich source — a soprano on a high note, a synth pad — can contain
/// 880 Hz and 1318 Hz simultaneously and would satisfy "both frequencies
/// appeared". Our chime does not: during note one, note two's bin is empty,
/// and vice versa. Requiring that contrast in each window is what turns "these
/// frequencies exist" into "our chime played".
pub const DETECT_CONTRAST: f32 = 4.0;

/// Milliseconds skipped at each end of a note before analysing it, so the
/// fades and any boundary smear stay out of the measurement.
const ANALYSIS_GUARD_MILLIS: u32 = 20;

/// How finely the detector searches for the chime's start.
///
/// The gap between "we handed samples to CoreAudio" and "they appear in the
/// tap" is device latency, tens of milliseconds and not known in advance. The
/// detector slides rather than assuming alignment.
const SEARCH_STEP_MILLIS: u32 = 5;

/// How long to keep listening for the chime before treating the probe as
/// failed (SPEC §8.1/A6; measured in TUR-4 by Tess against the spike's
/// single-tone equivalent, `spikes/phase0a-tcc/tone-probe-margin.py`).
///
/// A freshly-created tap does not settle onto real audio instantly. On the
/// fast path — no consent dialog, a warm tap, ~45 ms of setup — the measured
/// probe tone did not clear its detection threshold until **1.07 s** after
/// the device started. That first second is not silence (peak ≈0.41,
/// −24 dBFS, almost no exact-zero samples) — it is something else, so a
/// single check shortly after starting capture reads *denied* on a perfectly
/// granted system, which is the common case, not the rare one.
///
/// The fix is the same shape as the finding: poll, don't sample once. This
/// budget is ~2x the measured worst case, and [`probe`] uses it to decide how
/// long to keep pulling captured audio and repeating the chime before giving
/// up.
pub const ONSET_TIMEOUT_MILLIS: u32 = 2000;

/// Silence between repeats when the chime is replayed across
/// [`ONSET_TIMEOUT_MILLIS`], so consecutive repeats don't smear into each
/// other's analysis windows.
const REPEAT_GAP_MILLIS: u32 = 100;

/// How long the whole chime lasts.
pub fn duration_millis() -> u32 {
    NOTES.iter().map(|note| note.millis).sum()
}

/// The chime as mono samples in `-1.0..=1.0`, ready to render at
/// `sample_rate`.
pub fn samples(sample_rate: u32) -> Vec<f32> {
    let mut out =
        Vec::with_capacity((sample_rate as u64 * duration_millis() as u64 / 1000) as usize);
    for note in NOTES {
        let count = samples_in(note.millis, sample_rate);
        let fade = samples_in(FADE_MILLIS, sample_rate).min(count / 2);
        for index in 0..count {
            let seconds = index as f32 / sample_rate as f32;
            let wave = (std::f32::consts::TAU * note.hz * seconds).sin();
            out.push(AMPLITUDE * envelope(index, count, fade) * wave);
        }
    }
    out
}

/// The chime repeated back-to-back, with [`REPEAT_GAP_MILLIS`] of silence
/// between repeats, filling at least `total_millis`.
///
/// Used to play the chime across the whole [`ONSET_TIMEOUT_MILLIS`] settle
/// window rather than once, so at least one repeat lands after the tap has
/// settled onto real audio, wherever that boundary actually falls. A single
/// play timed to start immediately has no such guarantee — it can land
/// entirely inside the unsettled window the onset finding describes.
pub fn looped_samples(sample_rate: u32, total_millis: u32) -> Vec<f32> {
    let one = samples(sample_rate);
    let gap = vec![0.0f32; samples_in(REPEAT_GAP_MILLIS, sample_rate)];
    let period = one.len() + gap.len();
    let repeats = total_millis
        .div_ceil(duration_millis() + REPEAT_GAP_MILLIS)
        .max(1);

    let mut out = Vec::with_capacity(period * repeats as usize);
    for _ in 0..repeats {
        out.extend_from_slice(&one);
        out.extend_from_slice(&gap);
    }
    out
}

/// What the detector found, note by note.
///
/// The readings come back even when the verdict is negative, because the
/// difference between "we heard nothing at all" and "we heard something but it
/// was not ours" is the difference between a denial and a bug, and a log that
/// only records `false` cannot tell them apart.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// Whether the chime played and came back.
    pub present: bool,
    /// One entry per note, in order.
    pub notes: Vec<NoteReading>,
}

/// How one note measured, in the window where it should have been alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteReading {
    /// The note we were looking for.
    pub expected_hz: f32,
    /// Its level in that window, as a fraction of full scale.
    pub magnitude: f32,
    /// The *other* note's level in the same window. Should be near zero.
    pub other_magnitude: f32,
    /// Whether this note cleared both [`DETECT_FLOOR`] and [`DETECT_CONTRAST`].
    pub passed: bool,
}

/// Look for the chime in `captured`.
///
/// `captured` should start at or shortly before the moment the chime was
/// played and run at least [`duration_millis`] long; giving it some extra tail
/// costs nothing and absorbs device latency.
///
/// An absent chime means **the check did not pass**, which is not the same
/// sentence as "permission denied" — see the ⚠️ in the module docs. Callers
/// own that distinction.
pub fn heard(captured: &[f32], sample_rate: u32) -> Reading {
    let chime_len = samples_in(duration_millis(), sample_rate);
    if sample_rate == 0 || captured.len() < chime_len || chime_len == 0 {
        return Reading {
            present: false,
            notes: Vec::new(),
        };
    }

    let step = samples_in(SEARCH_STEP_MILLIS, sample_rate).max(1);
    let last_offset = captured.len() - chime_len;

    let mut best: Option<Reading> = None;
    let mut offset = 0;
    loop {
        let reading = read_at(captured, sample_rate, offset);
        let better = match &best {
            None => true,
            Some(current) => rank(&reading) > rank(current),
        };
        if better {
            let present = reading.present;
            best = Some(reading);
            // Nothing beats a clean pass, so stop looking.
            if present {
                break;
            }
        }
        if offset >= last_offset {
            break;
        }
        offset = (offset + step).min(last_offset);
    }

    best.expect("the loop always reads at least offset 0")
}

/// Poll for the chime instead of checking once.
///
/// `pull` returns the next chunk of freshly-captured audio, or `None` when
/// there is nothing left to read (the tap stopped, or the caller gave up).
/// Each chunk re-runs [`heard`] over everything captured so far — that is
/// what makes this a poll and not a single check, which is the mistake
/// [`ONSET_TIMEOUT_MILLIS`] documents. Stops as soon as the chime is heard,
/// or once [`ONSET_TIMEOUT_MILLIS`] worth of audio (plus one chime's length,
/// so the last pull gets a fair search) has been gathered.
///
/// Pair with [`looped_samples`] on the playback side: a single play timed to
/// start immediately can land entirely inside the window the onset finding
/// describes, with nothing this function does compensating for that.
pub fn probe(sample_rate: u32, mut pull: impl FnMut() -> Option<Vec<f32>>) -> Reading {
    let budget = samples_in(ONSET_TIMEOUT_MILLIS + duration_millis(), sample_rate);
    let mut captured = Vec::new();
    loop {
        let Some(chunk) = pull() else { break };
        captured.extend(chunk);

        let reading = heard(&captured, sample_rate);
        if reading.present || captured.len() >= budget {
            return reading;
        }
    }
    heard(&captured, sample_rate)
}

/// Score a reading so the search can pick the most convincing alignment. A
/// reading that passes outranks every one that does not; among failures, the
/// one whose weakest note read loudest is the most informative to log.
fn rank(reading: &Reading) -> (bool, OrderedF32) {
    let weakest = reading
        .notes
        .iter()
        .map(|note| note.magnitude)
        .fold(f32::INFINITY, f32::min);
    (reading.present, OrderedF32(weakest))
}

/// `f32` that can be compared, for ranking only. NaN sorts lowest.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedF32(f32);

impl PartialOrd for OrderedF32 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(match (self.0.is_nan(), other.0.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => self
                .0
                .partial_cmp(&other.0)
                .unwrap_or(std::cmp::Ordering::Equal),
        })
    }
}

/// Measure every note assuming the chime starts at `offset`.
fn read_at(captured: &[f32], sample_rate: u32, offset: usize) -> Reading {
    let guard = samples_in(ANALYSIS_GUARD_MILLIS, sample_rate);
    let mut notes = Vec::with_capacity(NOTES.len());
    let mut cursor = offset;

    for (index, note) in NOTES.iter().enumerate() {
        let len = samples_in(note.millis, sample_rate);
        let other = NOTES[(index + 1) % NOTES.len()].hz;

        // The steady middle of the note: past the fade in, short of the fade
        // out. If the note is too short to have a middle, fall back to all of
        // it rather than measuring nothing.
        let (start, end) = if len > 2 * guard {
            (cursor + guard, cursor + len - guard)
        } else {
            (cursor, cursor + len)
        };
        let window = &captured[start.min(captured.len())..end.min(captured.len())];

        let magnitude = goertzel(window, note.hz, sample_rate);
        let other_magnitude = goertzel(window, other, sample_rate);
        notes.push(NoteReading {
            expected_hz: note.hz,
            magnitude,
            other_magnitude,
            passed: magnitude >= DETECT_FLOOR && magnitude >= DETECT_CONTRAST * other_magnitude,
        });

        cursor += len;
    }

    Reading {
        present: notes.iter().all(|note| note.passed),
        notes,
    }
}

/// Level at `hz` in `window`, as a fraction of full scale: a pure full-scale
/// sine reads ≈ 1.0.
///
/// Goertzel rather than an FFT — we want two bins, not two thousand, and it is
/// the same analysis FINDINGS §8 used to verify the tap independently of the
/// probe's own meter. A Hann window goes on first: our frequencies do not land
/// on exact bin centres for an arbitrary window length, and unwindowed
/// Goertzel would lose up to ~4 dB to scalloping on a tone that is really
/// there. Hann costs a factor of two in coherent gain, which the `4.0` below
/// takes back out.
fn goertzel(window: &[f32], hz: f32, sample_rate: u32) -> f32 {
    let n = window.len();
    if n == 0 {
        return 0.0;
    }
    let omega = std::f64::consts::TAU * hz as f64 / sample_rate as f64;
    let coeff = 2.0 * omega.cos();

    let mut s_prev = 0.0f64;
    let mut s_prev2 = 0.0f64;
    for (index, sample) in window.iter().enumerate() {
        let hann = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / n as f64).cos();
        let s = *sample as f64 * hann + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }

    let power = s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2;
    (4.0 * power.max(0.0).sqrt() / n as f64) as f32
}

/// A raised-cosine fade in and out, `1.0` in between.
fn envelope(index: usize, count: usize, fade: usize) -> f32 {
    if fade == 0 {
        return 1.0;
    }
    let ramp =
        |position: usize| 0.5 - 0.5 * (std::f32::consts::PI * position as f32 / fade as f32).cos();
    if index < fade {
        ramp(index)
    } else if index + fade >= count {
        ramp(count.saturating_sub(index + 1))
    } else {
        1.0
    }
}

fn samples_in(millis: u32, sample_rate: u32) -> usize {
    (sample_rate as u64 * millis as u64 / 1000) as usize
}

#[cfg(test)]
mod tests {
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
}
