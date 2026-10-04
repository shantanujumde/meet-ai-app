//! Is the default output speakers or headphones? (TUR-65, SPEC L6.)
//!
//! Without headphones the mic hears the other people through the speakers,
//! so both tracks hold the same voices and the transcript repeats lines.
//! SPEC L6 says detect and warn, never fix in code; the app shows a quiet
//! banner when [`should_warn`] says so.
//!
//! [`classify`] is pure and tested on every OS over fake [`OutputDevice`]s.
//! Reading the device is OS code, in `platform/headphones/`: Core Audio's
//! transport type, data source and stream terminal types on macOS, the
//! endpoint form factor and the adapter's bus on Windows, and the default
//! sink's active port and properties on Linux. Anything the OS does not say
//! clearly is [`OutputKind::Unknown`], and unknown never warns: a wrong
//! warning on every recording is worse than a missing one.
//!
//! The name lists follow anarlog's `is_headphone` heuristics
//! (`crates/audio-device`, MIT); see THIRD_PARTY_NOTICES.md.

/// How the output device is connected, as far as [`classify`] cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// The computer's own audio hardware (speakers or its headphone jack).
    BuiltIn,
    /// Bluetooth Classic or Bluetooth LE.
    Bluetooth,
    Usb,
    /// HDMI or DisplayPort: a monitor or TV.
    Display,
    /// AirPlay: a speaker, a TV or another computer on the network.
    AirPlay,
    /// An aggregate, a virtual or loopback device, a remote session.
    Virtual,
    /// Anything else, or not known.
    Other,
}

/// What the OS itself says the endpoint is, before any name guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Headphones,
    /// Headphones with a mic, or a phone-style handset.
    Headset,
    Speakers,
    /// The OS did not say, or said something else (line out, S/PDIF).
    Unknown,
}

/// The default output, as the OS describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// The name the OS shows the user.
    pub name: String,
    pub transport: Transport,
    pub form: Form,
}

/// The answer: what the user is listening on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    Speakers,
    Headphones,
    /// No reading, or not clear enough to act on. Never warns.
    Unknown,
}

/// Whether to show the "No headphones" banner: only for speakers, and only
/// with `audio.warn_no_headphones` on.
pub fn should_warn(kind: OutputKind, warn_no_headphones: bool) -> bool {
    warn_no_headphones && kind == OutputKind::Speakers
}

/// What `device` is. `None` (no default output, or the read failed) is
/// unknown.
///
/// The order matters:
/// 1. What the OS says the endpoint is wins.
/// 2. Bluetooth is headphones unless the name says speaker: most Bluetooth
///    audio on a laptop is headphones, and popular ones (Sony WH-*, Bose
///    QC*) carry no headphone word in their name.
/// 3. A name that says headphones or speaker.
/// 4. AirPlay plays to a speaker, a TV or another computer, none of which
///    the user wears.
/// 5. Everything else is unknown: a built-in output with no data source, a
///    USB DAC, a monitor (which may have headphones in its jack), a virtual
///    device.
pub fn classify(device: Option<&OutputDevice>) -> OutputKind {
    let Some(device) = device else {
        return OutputKind::Unknown;
    };
    match device.form {
        Form::Headphones | Form::Headset => return OutputKind::Headphones,
        Form::Speakers => return OutputKind::Speakers,
        Form::Unknown => {}
    }
    if device.transport == Transport::Bluetooth {
        return if name_suggests_speaker(&device.name) {
            OutputKind::Speakers
        } else {
            OutputKind::Headphones
        };
    }
    if name_suggests_headphones(&device.name) {
        return OutputKind::Headphones;
    }
    if name_suggests_speaker(&device.name) {
        return OutputKind::Speakers;
    }
    if device.transport == Transport::AirPlay {
        return OutputKind::Speakers;
    }
    OutputKind::Unknown
}

/// The default output's kind, read now. A failed read is logged and is
/// [`OutputKind::Unknown`], so it never warns. Property reads only: nothing
/// is opened, so this never prompts or changes what the user hears.
pub fn default_output_kind() -> OutputKind {
    match crate::platform::default_output_info() {
        Ok(device) => {
            let kind = classify(device.as_ref());
            tracing::debug!(?device, ?kind, "default output read");
            kind
        }
        Err(error) => {
            tracing::debug!(%error, "could not read the default output; not warning");
            OutputKind::Unknown
        }
    }
}

/// A name that says headphones, a headset or earbuds.
pub(crate) fn name_suggests_headphones(name: &str) -> bool {
    const MARKERS: &[&str] = &[
        "headphone",
        "headset",
        "earphone",
        "earbud",
        "airpods",
        "hands-free",
        "handsfree",
    ];
    contains_any(name, MARKERS)
}

/// Bluetooth and USB speakers by name, so they are not taken for
/// headphones.
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/device.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(crate) fn name_suggests_speaker(name: &str) -> bool {
    const MARKERS: &[&str] = &[
        "speaker",
        "soundbar",
        "sound bar",
        "soundlink",
        "boombox",
        "homepod",
        "sonos",
        "megaboom",
        "wonderboom",
        "jbl flip",
        "jbl charge",
        "jbl clip",
        "jbl go",
        "jbl xtreme",
    ];
    contains_any(name, MARKERS)
}

fn contains_any(name: &str, markers: &[&str]) -> bool {
    let lower = name.to_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests;
