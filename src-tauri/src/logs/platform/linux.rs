//! The Linux half of a native crash note: the signal that killed us.

use std::fmt;

/// `signal 11, code 1`, without allocating.
pub fn describe(context: &crash_handler::CrashContext, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let info = &context.siginfo;
    write!(f, "signal {}, code {}", info.ssi_signo, info.ssi_code)
}
