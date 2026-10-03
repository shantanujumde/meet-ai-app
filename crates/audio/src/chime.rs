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

mod attempts;

pub use attempts::{
    Attempt, LISTEN_TAIL_MILLIS, MAX_PLAYS, SETTLE_MILLIS, play_until_heard, worst_case_millis,
};
pub use attempts::{
    Clock, DEADLINE_HEADROOM_MILLIS, Ended, FIRST_FRAME_TIMEOUT_MILLIS, Finish, Listened,
    WallClock, listen_live,
};

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
/// budget is ~2x the measured worst case. [`play_until_heard`] plays the chime
/// once after the tap has settled, and if that play goes unheard it uses this
/// budget to time its single retry: the second play never starts before this
/// much audio has been captured, so it lands well past any settle time seen
/// so far. [`probe`] still uses it as its pull budget for the hardware tests.
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
/// Plays the chime across the whole [`ONSET_TIMEOUT_MILLIS`] settle window
/// rather than once, so at least one repeat lands after the tap has settled
/// onto real audio, wherever that boundary actually falls.
///
/// Kept for the `#[ignore]`d hardware closed-loop tests
/// (`crates/audio/tests/system_closed_loop.rs`, `mic_closed_loop.rs`). The
/// app's recording-start check does not use it any more: about eleven chimes
/// per recording was the TUR-14 bug. The app uses [`play_until_heard`], which
/// waits for the tap to settle and plays once (twice at most).
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
///
/// Kept as the listen-only half of the looped approach that the `#[ignore]`d
/// hardware closed-loop tests (`crates/audio/tests/system_closed_loop.rs`,
/// `mic_closed_loop.rs`) still play with [`looped_samples`]. The app's check
/// uses [`play_until_heard`] instead, which owns playback too, plays the
/// chime once and listens while it plays.
pub fn probe(sample_rate: u32, mut pull: impl FnMut() -> Option<Vec<f32>>) -> Reading {
    let budget = samples_in(ONSET_TIMEOUT_MILLIS + duration_millis(), sample_rate);
    let mut captured = Vec::new();
    while let Some(chunk) = pull() {
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
mod tests;
