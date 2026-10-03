//! The real audio-permission measurement (SPEC §8.1/A6).
//!
//! `src-tauri/src/permission.rs` owns the onboarding screen, not the
//! measurement — its own doc comment says the measurement "belongs to
//! `crates/audio`". This module is that measurement, built out of the same
//! pieces [`crate::chime`] and the closed-loop tests already verify against
//! real hardware (`tests/system_closed_loop.rs`, `tests/mic_closed_loop.rs`):
//! it just runs them as a one-shot check instead of an `#[ignore]`d test.
//!
//! The two channels fail differently, so they are checked differently:
//!
//! - **Microphone**: [`check_mic`] first asks the public, synchronous
//!   `AVCaptureDevice.authorizationStatus(for: .audio)` (SPEC §8.1 names this
//!   as the mic's likely exemption from the tap's "every OSStatus lies"
//!   problem, but left it unverified). **TUR-127 verified it the hard way**:
//!   a stored **Denied** decision lets `cpal`'s `build_input_stream` /
//!   `AudioUnitInitialize` open cleanly and a recording capture real,
//!   non-zero audio anyway — indistinguishable, by cpal's return code alone,
//!   from a genuine grant. So a `Denied`/`Restricted` answer from
//!   `AVCaptureDevice` is now trusted outright and the mic is never opened;
//!   only `NotDetermined` (no stored decision yet) falls through to the
//!   open-and-see probe below — which is what actually triggers the OS
//!   consent dialog — and [`crate::AUDIO_PERMISSION_TIMEOUT`] still turns
//!   "nobody answered that dialog" into [`crate::Error::PermissionDenied`]
//!   rather than a hang, see `MicSource::build`'s doc comment.
//! - **System audio**: FINDINGS §10.1 measured that a denied tap returns
//!   `noErr` and bit-exact zero samples at the normal callback rate, so a
//!   return code proves nothing. [`check_system`] plays the permission chime
//!   through the default output device and confirms [`crate::chime::heard`]
//!   recovers it from the tap, listening live. A chime that is not
//!   recognised but comes back with real, non-zero audio still proves the
//!   grant (TUR-84): only exact zeros after a full listen are a denial, see
//!   [`verdict::outcome`]. One chime per check (TUR-14:
//!   it used to loop for ~3 s and sound like ~11 chimes): it waits for the
//!   tap to settle, plays once, and stops as soon as the chime is heard. Only
//!   if it is not heard does it play again, and never more than
//!   [`crate::chime::MAX_PLAYS`] times in all.
//!
//! Both functions are on-demand and take real wall-clock time (system audio:
//! the chime starts ~[`crate::chime::SETTLE_MILLIS`] (1.2 s) after the tap on
//! a normal start, and the whole check is bounded by about
//! [`crate::chime::worst_case_millis`]; mic: near-instant unless a dialog is
//! open). Callers must run them off the UI thread — see
//! `src-tauri/src/commands.rs`'s `permission_status`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{AudioSource, Error, chime, platform};

pub mod verdict;

/// Where one channel's check landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelState {
    /// The positive control came back (or, for the mic, the stream opened).
    Granted,
    /// The tone did not come back and only exact zeros did (TUR-84), or the
    /// stream was refused.
    Denied,
    /// The check itself could not run — no device, no output, an I/O error.
    /// This is neither a grant nor a denial; a caller that folds it into
    /// "denied" would send a user with no output device to instructions that
    /// cannot help them.
    Unmeasurable,
}

/// One channel's result, with the sentence the onboarding screen can show.
#[derive(Debug, Clone)]
pub struct ChannelResult {
    pub state: ChannelState,
    pub detail: String,
}

fn scratch_dir(label: &str) -> Result<(PathBuf, PathBuf), String> {
    let dir = std::env::temp_dir().join(format!(
        "meet-ai-permission-check-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let dest = dir.join(format!("{label}.wav"));
    let _ = std::fs::remove_file(&dest);
    Ok((dir, dest))
}

fn cleanup(dir: &std::path::Path, dest: &std::path::Path) {
    let _ = std::fs::remove_file(dest);
    let _ = std::fs::remove_dir(dir);
}

/// The microphone's stored decision alone: nothing opened, nothing played.
///
/// The instant, silent half of [`check_mic`], for a caller that must not make
/// a sound — the app runs this every time it starts, and TUR-24 (SPEC A7)
/// keeps the chime to setup and to the start of a recording. A stored denial
/// is as authoritative here as it is there; anything else is
/// [`ChannelState::Unmeasurable`], never `Granted`, because nothing was opened
/// to prove it.
pub fn mic_decision() -> ChannelResult {
    platform::stored_mic_denial().unwrap_or_else(|| ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: "the microphone has not been opened since meet-ai started".into(),
    })
}

/// Start the mic briefly and see whether Core Audio lets it through.
///
/// Bounded by [`crate::AUDIO_PERMISSION_TIMEOUT`] inside `MicSource::start`:
/// worst case this blocks that long waiting for a dialog nobody answers.
pub fn check_mic() -> ChannelResult {
    if let Some(denial) = platform::stored_mic_denial() {
        return denial;
    }
    // `Authorized` and `NotDetermined` both fall through: `Authorized` still
    // opens the stream below to also confirm a device exists, `NotDetermined`
    // opens it because that is what triggers the OS consent dialog in the
    // first place.
    platform::check_mic()
}

/// The open-and-see half of [`check_mic`]: start `source` briefly and report
/// whether it opened. The platform seam calls this with its microphone
/// (`crate::mic::MicSource` on every OS so far).
pub fn check_mic_with(mut source: Box<dyn AudioSource>) -> ChannelResult {
    let (dir, dest) = match scratch_dir("mic") {
        Ok(paths) => paths,
        Err(error) => {
            return ChannelResult {
                state: ChannelState::Unmeasurable,
                detail: format!(
                    "could not create a scratch folder to test the microphone: {error}"
                ),
            };
        }
    };

    let result = match source.start(dest.clone()) {
        Ok(()) => {
            let _ = source.stop();
            ChannelResult {
                state: ChannelState::Granted,
                detail: "the microphone stream opened".into(),
            }
        }
        Err(Error::PermissionDenied) => ChannelResult {
            state: ChannelState::Denied,
            detail: "the microphone did not open — permission is off, or nobody answered the \
                     prompt in time"
                .into(),
        },
        Err(other) => ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: format!("could not test the microphone: {other}"),
        },
    };
    cleanup(&dir, &dest);
    result
}

/// Play the chime through the default output device and confirm the system
/// tap recovers it. Mirrors `tests/system_closed_loop.rs`'s closed loop.
///
/// One chime per check. The tap's frames are copied live through a
/// [`crate::tee`], and [`chime::play_until_heard`] drives the timing: it
/// waits [`chime::SETTLE_MILLIS`] (~1.2 s) of captured audio for the tap to
/// settle, plays the chime once, listens, and returns as soon as it is
/// heard. If it is not, it plays once more — at most [`chime::MAX_PLAYS`]
/// plays in all — so a denied tap costs about [`chime::worst_case_millis`].
/// A wall-clock deadline a little past that, counted from the tap's first
/// frame, ends the check if the tap stops delivering audio; a check cut
/// short that way is unmeasurable, never denied (TUR-72).
///
/// [`ChannelState::Unmeasurable`] on a platform with no system-audio capture
/// yet (`crate::platform`).
pub fn check_system() -> ChannelResult {
    platform::check_system()
}

/// [`check_system`]'s closed loop against `system`, a system-audio source
/// that has not been started. The platform seam calls this with its tap.
pub fn check_system_with(mut system: Box<dyn AudioSource>) -> ChannelResult {
    let host = cpal::default_host();
    let Some(output) = host.default_output_device() else {
        return ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: "this Mac has no default output device to play the check tone through".into(),
        };
    };
    let output_config = match output.default_output_config() {
        Ok(config) => config.config(),
        Err(error) => {
            return ChannelResult {
                state: ChannelState::Unmeasurable,
                detail: format!("the default output device has no usable config: {error}"),
            };
        }
    };
    let output_rate = output_config.sample_rate;
    let output_channels = (output_config.channels as usize).max(1);

    let (dir, dest) = match scratch_dir("system") {
        Ok(paths) => paths,
        Err(error) => {
            return ChannelResult {
                state: ChannelState::Unmeasurable,
                detail: format!("could not create a scratch folder to test system audio: {error}"),
            };
        }
    };

    // A live copy of the tap's 16 kHz mono frames, so the check can listen
    // while it plays instead of reading the WAV back afterwards.
    let (tee, feed) = crate::tee::tee();
    system.tee(tee);
    if let Err(error) = system.start(dest.clone()) {
        cleanup(&dir, &dest);
        return ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: format!("could not start the system-audio tap: {error}"),
        };
    }

    // The chime, rendered once at the output device's rate. The output
    // callback plays it from the top each time `play` asks, then falls silent
    // until asked again — one chime per play, never a loop.
    let mono = chime::samples(output_rate);
    let restart = Arc::new(AtomicBool::new(false));
    let callback_restart = Arc::clone(&restart);
    // Start past the end: silence until the first play request.
    let mut position = mono.len();

    let output_stream = output.build_output_stream(
        output_config,
        move |data: &mut [f32], _| {
            // `swap` reads and clears the request in one step, so a play asked
            // for mid-buffer starts on the next buffer, exactly once.
            if callback_restart.swap(false, Ordering::AcqRel) {
                position = 0;
            }
            let frames = data.len() / output_channels;
            for (i, frame) in data.chunks_mut(output_channels).enumerate() {
                let sample = mono.get(position + i).copied().unwrap_or(0.0);
                for s in frame {
                    *s = sample;
                }
            }
            position = position.saturating_add(frames);
        },
        |error| tracing::warn!(%error, "permission-check output stream error"),
        None,
    );
    let output_stream = match output_stream {
        Ok(stream) => stream,
        Err(error) => {
            let _ = system.stop();
            cleanup(&dir, &dest);
            return ChannelResult {
                state: ChannelState::Unmeasurable,
                detail: format!("could not build the check-tone output stream: {error}"),
            };
        }
    };
    if let Err(error) = output_stream.play() {
        let _ = system.stop();
        cleanup(&dir, &dest);
        return ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: format!("could not start playback of the check tone: {error}"),
        };
    }

    // Listen to the tap live instead of sleeping and reading the WAV back.
    // `play_until_heard` keeps time by captured samples; `listen_live` adds
    // the wall-clock safety net, counted from the tap's first frame so a
    // slow start cannot eat the listen time (TUR-72).
    let clock = chime::WallClock::start();
    let play = || restart.store(true, Ordering::Release);
    let listened = chime::listen_live(verdict::RATE, &clock, play, |left| feed.recv_timeout(left));

    drop(output_stream);
    let _ = system.stop();

    verdict::log(&listened);
    let result = verdict::system_verdict(&listened);

    cleanup(&dir, &dest);
    result
}
