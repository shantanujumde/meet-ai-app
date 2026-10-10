//! Core Audio property reads, once (TUR-177): a fixed-size value
//! ([`read`]), a list of object ids ([`read_array`]) and a `CFString`
//! ([`read_cf_string`]). Every read checks the size Core Audio says it
//! wrote, so a value is only handed back when a whole one arrived.

use std::fmt;
use std::ptr::NonNull;

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_foundation::{CFRetained, CFString};

/// Why a property read gave nothing back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadError {
    /// Core Audio's own error code.
    Status(i32),
    /// `noErr`, but a different number of bytes than the value's size.
    Size { expected: usize, got: usize },
    /// A `CFString` property that held a null reference.
    Null,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(status) => write!(f, "OSStatus {status}"),
            Self::Size { expected, got } => {
                write!(f, "wrote {got} bytes where {expected} were expected")
            }
            Self::Null => f.write_str("returned a null value"),
        }
    }
}

/// The address of `selector` in `scope`, on the main element.
pub(crate) fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: ca::kAudioObjectPropertyElementMain,
    }
}

/// A read's status and written size to a result: `noErr` with exactly
/// `expected` bytes is the only success.
fn checked(status: i32, got: usize, expected: usize) -> Result<(), ReadError> {
    if status != 0 {
        return Err(ReadError::Status(status));
    }
    if got != expected {
        return Err(ReadError::Size { expected, got });
    }
    Ok(())
}

/// A whole number of `T`s in `got` bytes, or the size error that says not.
fn element_count<T>(got: usize) -> Result<usize, ReadError> {
    let size = std::mem::size_of::<T>();
    if size == 0 || !got.is_multiple_of(size) {
        return Err(ReadError::Size {
            expected: got.next_multiple_of(size.max(1)),
            got,
        });
    }
    Ok(got / size)
}

/// One fixed-size property of `object`.
///
/// # Safety
/// `T` must match the C layout of `selector`'s value on `object`
/// (`AudioObjectID` for a device-id property, `AudioStreamBasicDescription`
/// for a format, a raw pointer for a `CFStringRef`).
pub(crate) unsafe fn read<T: Copy>(
    object: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Result<T, ReadError> {
    let mut addr = address(selector, scope);
    let expected = std::mem::size_of::<T>();
    let mut size = expected as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    // SAFETY: valid address, size and out-pointer for the duration of the
    // call; no qualifier is passed (size 0, null pointer).
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    checked(status, size as usize, expected)?;
    // SAFETY: `noErr` with the full size written means Core Audio filled a `T`.
    Ok(unsafe { value.assume_init() })
}

/// A list-valued property of `object` (device, stream or process ids),
/// empty when the list is. The list can shrink between the size query and
/// the read; the shorter one is returned.
pub(crate) fn read_array(
    object: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Result<Vec<AudioObjectID>, ReadError> {
    let mut addr = address(selector, scope);
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer for the duration of the call.
    let status = unsafe {
        ca::AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return Err(ReadError::Status(status));
    }
    let count = element_count::<AudioObjectID>(size as usize)?;
    let mut ids = vec![ca::kAudioObjectUnknown; count];
    if count == 0 {
        return Ok(ids);
    }
    // SAFETY: `ids` holds `size` bytes of `AudioObjectID`s and outlives the
    // call; Core Audio writes at most `size` bytes and updates `size`.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut ids[0]).cast(),
        )
    };
    if status != 0 {
        return Err(ReadError::Status(status));
    }
    let written = element_count::<AudioObjectID>(size as usize)?;
    ids.truncate(written.min(count));
    Ok(ids)
}

/// A `CFStringRef`-valued property of `object` (a UID, a name, a bundle
/// id). Core Audio hands these over with a +1 retain (its "copy" rule);
/// the string is released once read.
pub(crate) fn read_cf_string(
    object: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Result<String, ReadError> {
    // SAFETY: each caller names a `CFStringRef` property, which is a pointer.
    let raw: *mut CFString = unsafe { read(object, selector, scope)? };
    let ptr = NonNull::new(raw).ok_or(ReadError::Null)?;
    // SAFETY: a non-null `CFString` handed over with a +1 retain that this
    // `CFRetained` now owns and releases.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(ptr) };
    Ok(owned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: AudioObjectID = ca::kAudioObjectSystemObject as AudioObjectID;
    const GLOBAL: u32 = ca::kAudioObjectPropertyScopeGlobal;

    #[test]
    fn only_no_error_with_the_full_size_is_a_value() {
        assert_eq!(checked(0, 4, 4), Ok(()));
        assert_eq!(checked(-50, 4, 4), Err(ReadError::Status(-50)));
        assert_eq!(
            checked(0, 2, 4),
            Err(ReadError::Size {
                expected: 4,
                got: 2
            })
        );
    }

    #[test]
    fn a_list_must_be_whole_ids() {
        assert_eq!(element_count::<AudioObjectID>(0), Ok(0));
        assert_eq!(element_count::<AudioObjectID>(12), Ok(3));
        assert_eq!(
            element_count::<AudioObjectID>(6),
            Err(ReadError::Size {
                expected: 8,
                got: 6
            })
        );
    }

    #[test]
    fn errors_read_as_the_old_log_text() {
        assert_eq!(ReadError::Status(-50).to_string(), "OSStatus -50");
    }

    /// Reads properties of whatever this machine has, opens nothing (like
    /// `device_watch`'s smoke test).
    #[test]
    fn the_system_object_reads_on_this_machine() {
        // SAFETY: the default output device is an `AudioObjectID`.
        let device: Result<AudioObjectID, _> = unsafe {
            read(
                SYSTEM,
                ca::kAudioHardwarePropertyDefaultOutputDevice,
                GLOBAL,
            )
        };
        assert!(device.is_ok(), "{device:?}");
        let devices = read_array(SYSTEM, ca::kAudioHardwarePropertyDevices, GLOBAL);
        assert!(devices.is_ok(), "{devices:?}");
    }

    /// A value of the wrong size never comes back as one, whatever Core
    /// Audio answers (an error status or a short write).
    #[test]
    fn a_wrong_sized_read_is_an_error() {
        // SAFETY: a `u64` buffer is valid memory for Core Audio to write the
        // 4-byte device id into; the size check is what is under test.
        let wide: Result<u64, _> = unsafe {
            read(
                SYSTEM,
                ca::kAudioHardwarePropertyDefaultOutputDevice,
                GLOBAL,
            )
        };
        assert!(wide.is_err(), "{wide:?}");
    }
}
