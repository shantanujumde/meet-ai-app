//! The macOS half of a native crash note: the Mach exception that killed us.

use std::fmt;

/// `EXC_BAD_ACCESS (kind 1), code 0x1, subcode 0x0`, without allocating.
pub fn describe(context: &crash_handler::CrashContext, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let Some(info) = context.exception else {
        return f.write_str("unknown");
    };
    write!(
        f,
        "{} (kind {}), code {:#x}",
        exception_name(info.kind),
        info.kind,
        info.code
    )?;
    if let Some(subcode) = info.subcode {
        write!(f, ", subcode {subcode:#x}")?;
    }
    Ok(())
}

/// The `EXC_*` name for a Mach exception kind, from `<mach/exception_types.h>`.
fn exception_name(kind: u32) -> &'static str {
    match kind {
        1 => "EXC_BAD_ACCESS",
        2 => "EXC_BAD_INSTRUCTION",
        3 => "EXC_ARITHMETIC",
        4 => "EXC_EMULATION",
        5 => "EXC_SOFTWARE",
        6 => "EXC_BREAKPOINT",
        10 => "EXC_CRASH",
        11 => "EXC_RESOURCE",
        12 => "EXC_GUARD",
        _ => "unknown exception",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mach_exception_kinds_have_their_header_names() {
        assert_eq!(super::exception_name(1), "EXC_BAD_ACCESS");
        assert_eq!(super::exception_name(10), "EXC_CRASH");
        assert_eq!(super::exception_name(99), "unknown exception");
    }
}
