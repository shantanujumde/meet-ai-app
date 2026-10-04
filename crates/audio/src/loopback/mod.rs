//! System audio recorded from an output device's loopback (TUR-37): the
//! shared half, which builds and is tested on every OS.
//!
//! Windows records "what the speakers play" as a `cpal` input stream on the
//! default output device (WASAPI loopback, `AUDCLNT_STREAMFLAGS_LOOPBACK`).
//! That stream has four habits the macOS process tap does not, and each has
//! a pure answer here, so it runs (and is tested) on every OS while only the
//! WASAPI wiring lives in `crate::platform` (rule R10):
//!
//! * It sends nothing while nothing plays. The Windows side keeps a render
//!   stream of zeros open on the same device ([`source::Backend::start_keepalive`]),
//!   and [`clock::Timeline`] turns any jump in the packets' capture times into
//!   silence, so the track keeps wall time even when the keepalive fails.
//! * `cpal` gives no device position, only a capture time per packet: the
//!   [`clock`] maths turns those into the `(host_ns, frames)` pairs
//!   [`crate::AudioSource::position`] reports, in the same QPC domain as the
//!   microphone's (`crate::platform::input_callback_ns`).
//! * WASAPI marks some buffers `AUDCLNT_BUFFERFLAGS_SILENT`, whose bytes are
//!   to be ignored; [`silent::condition`] zero-fills those (and any non-finite
//!   sample, which would poison the resampler for good).
//! * A default-device switch (a headset plugged in) is followed by the OS, not
//!   by us: [`follower::DeviceWatch`] decides when the switch is real, and
//!   the session then opens a new segment (`session::reopen_segment`).
//!
//! [`source::LoopbackSource`] puts these together into an [`crate::AudioSource`]
//! over a [`source::Backend`], the one OS-specific piece. Linux (TUR-38) can
//! reuse all of it with a PipeWire or PulseAudio monitor backend.
//!
//! Public so the OS backends can reach it without a dead-code allowance on
//! the OSes that do not use it yet.

/// Capture buffer sizing for a loopback stream.
pub mod buffer;

/// The capture callback's side: one OS packet in, samples and gaps out.
pub mod capture;

/// Capture timestamps to a monotonic timeline, and the gaps in it.
pub mod clock;

/// When a default-device change is real: the switch policy.
pub mod follower;

/// Zero-filling buffers the OS marked silent, and non-finite samples.
pub mod silent;

/// The loopback [`crate::AudioSource`] over an OS [`source::Backend`].
pub mod source;

/// Gap marks between the capture callback and the worker thread.
mod splice;
