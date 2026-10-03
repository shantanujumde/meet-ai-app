//! The fresh UUID every process tap's `CATapDescription` needs, moved out of
//! `tap.rs` as is (TUR-84) to keep that file under the size limit.

/// Not cryptographically random — only needs to be unique among this
/// process's own private, per-recording taps, which a mix of wall-clock
/// nanoseconds and a process-wide counter comfortably provides. Avoids
/// pulling in a `uuid`/`rand` dependency for the one field
/// `CATapDescription` needs a fresh value in.
pub(super) fn locally_unique_uuid_bytes() -> [u8; 16] {
    use std::sync::atomic::{AtomicU64 as Counter, Ordering as CounterOrdering};
    static COUNTER: Counter = Counter::new(0);
    let counter = COUNTER.fetch_add(1, CounterOrdering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mixed = nanos ^ ((pid as u128) << 64) ^ (counter as u128);
    mixed.to_le_bytes()
}

/// Formats 16 bytes as a canonical `8-4-4-4-12` hex UUID string.
///
/// `objc2-foundation` 0.3.2's `NSUUID::from_bytes`/`initWithUUIDBytes:` is
/// documented by the crate itself as requiring the `disable-encoding-
/// assertions` feature to use at all: `__NSConcreteUUID`'s real method
/// signature takes a `char*`, not the inline 16-byte array the public
/// headers claim, so calling it with encoding assertions on panics at the
/// Objective-C message-send boundary — confirmed by reproducing it directly
/// against a live tap (TUR-4). Going through `initWithUUIDString:` instead
/// (via [`NSUUID::from_string`]) sidesteps that mismatched-encoding method
/// entirely rather than weakening encoding verification crate-wide for one
/// call site.
pub(super) fn format_uuid_bytes(bytes: [u8; 16]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locally_unique_uuid_bytes_do_not_repeat_back_to_back() {
        let a = locally_unique_uuid_bytes();
        let b = locally_unique_uuid_bytes();
        assert_ne!(a, b);
    }
}
