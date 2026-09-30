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
//!   recovers it from a live tap recording.
//!
//! Both functions are on-demand and take real wall-clock time (system audio:
//! at least [`crate::chime::ONSET_TIMEOUT_MILLIS`] worth; mic: near-instant
//! unless a dialog is open). Callers must run them off the UI thread — see
//! `src-tauri/src/commands.rs`'s `permission_status`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::mic::MicSource;
use crate::{AudioSource, Error, chime};

#[cfg(target_os = "macos")]
use crate::macos::tap::SystemSource;

/// Where one channel's check landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelState {
    /// The positive control came back (or, for the mic, the stream opened).
    Granted,
    /// The tone did not come back, or the stream was refused.
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

/// Ask macOS directly whether the microphone is authorized, without opening
/// any stream.
///
/// `AVCaptureDevice.authorizationStatus(for: .audio)` is a public, documented,
/// synchronous TCC query — a different mechanism entirely from the process
/// tap's `OSStatus`/sample-payload path that FINDINGS §10.1 proved lies on
/// denial. It reads the same `kTCCServiceMicrophone` record `cpal`'s
/// CoreAudio path is ultimately gated by, for this same process, so a
/// `Denied`/`Restricted` answer here is authoritative.
#[cfg(target_os = "macos")]
fn mic_authorization_status() -> objc2_av_foundation::AVAuthorizationStatus {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: `AVMediaTypeAudio` is an Apple-provided static that is always
    // present once the AVFoundation image is loaded; `authorizationStatusForMediaType`
    // reads TCC state and has no other preconditions.
    unsafe {
        let media_type =
            AVMediaTypeAudio.expect("AVFoundation always provides the AVMediaTypeAudio constant");
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
    }
}

/// A stored `Denied`/`Restricted` microphone decision, if there is one.
///
/// `None` covers `Authorized` and `NotDetermined` alike: neither says anything
/// about the mic until a stream is actually opened.
fn stored_mic_denial() -> Option<ChannelResult> {
    #[cfg(target_os = "macos")]
    {
        use objc2_av_foundation::AVAuthorizationStatus;
        let status = mic_authorization_status();
        if status == AVAuthorizationStatus::Denied {
            return Some(ChannelResult {
                state: ChannelState::Denied,
                detail: "macOS reports the microphone permission as explicitly denied \
                         (AVAuthorizationStatusDenied)"
                    .into(),
            });
        }
        if status == AVAuthorizationStatus::Restricted {
            return Some(ChannelResult {
                state: ChannelState::Denied,
                detail: "macOS reports the microphone as restricted (parental controls or an \
                         MDM profile), which this client cannot change"
                    .into(),
            });
        }
    }
    None
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
    stored_mic_denial().unwrap_or_else(|| ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: "the microphone has not been opened since meet-ai started".into(),
    })
}

/// Start the mic briefly and see whether Core Audio lets it through.
///
/// Bounded by [`crate::AUDIO_PERMISSION_TIMEOUT`] inside `MicSource::start`:
/// worst case this blocks that long waiting for a dialog nobody answers.
pub fn check_mic() -> ChannelResult {
    if let Some(denial) = stored_mic_denial() {
        return denial;
    }
    // `Authorized` and `NotDetermined` both fall through: `Authorized` still
    // opens the stream below to also confirm a device exists, `NotDetermined`
    // opens it because that is what triggers the OS consent dialog in the
    // first place.

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

    let mut source = MicSource::new();
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
#[cfg(target_os = "macos")]
pub fn check_system() -> ChannelResult {
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

    let mut system = SystemSource::new();
    if let Err(error) = system.start(dest.clone()) {
        cleanup(&dir, &dest);
        return ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: format!("could not start the system-audio tap: {error}"),
        };
    }

    let total_millis = chime::ONSET_TIMEOUT_MILLIS + chime::duration_millis() + 1000;
    let mono = chime::looped_samples(output_rate, total_millis);
    let cursor = Arc::new(AtomicUsize::new(0));
    let play_cursor = Arc::clone(&cursor);

    let output_stream = output.build_output_stream(
        output_config,
        move |data: &mut [f32], _| {
            let start = play_cursor.load(Ordering::Relaxed);
            for (i, frame) in data.chunks_mut(output_channels).enumerate() {
                let sample = mono.get(start + i).copied().unwrap_or(0.0);
                for s in frame {
                    *s = sample;
                }
            }
            play_cursor.fetch_add(data.len() / output_channels, Ordering::Relaxed);
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

    std::thread::sleep(Duration::from_millis(u64::from(total_millis) + 300));
    drop(output_stream);
    let _ = system.stop();

    let result = match hound::WavReader::open(&dest) {
        Ok(mut reader) => {
            let samples: Vec<f32> = reader
                .samples::<i16>()
                .filter_map(Result::ok)
                .map(|s| f32::from(s) / f32::from(i16::MAX))
                .collect();
            if samples.is_empty() {
                ChannelResult {
                    state: ChannelState::Unmeasurable,
                    detail: "the system-audio check recording was empty".into(),
                }
            } else {
                let reading = chime::heard(&samples, 16_000);
                if reading.present {
                    ChannelResult {
                        state: ChannelState::Granted,
                        detail: "the check tone was played and recovered from the system-audio \
                                 recording"
                            .into(),
                    }
                } else {
                    ChannelResult {
                        state: ChannelState::Denied,
                        detail: "the check tone did not come back through the system-audio tap"
                            .into(),
                    }
                }
            }
        }
        Err(error) => ChannelResult {
            state: ChannelState::Unmeasurable,
            detail: format!("the system-audio check recording could not be read: {error}"),
        },
    };

    cleanup(&dir, &dest);
    result
}

#[cfg(not(target_os = "macos"))]
pub fn check_system() -> ChannelResult {
    ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: "system-audio capture is not implemented on this platform yet".into(),
    }
}
