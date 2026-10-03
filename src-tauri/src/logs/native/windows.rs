//! The Windows half of a native crash note: the structured exception code.

use std::fmt;

/// `exception code 0xc0000005`: the unsigned NTSTATUS everyone searches for.
pub fn describe(context: &crash_handler::CrashContext, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
        f,
        "exception code {:#010x}",
        context.exception_code.cast_unsigned()
    )
}
