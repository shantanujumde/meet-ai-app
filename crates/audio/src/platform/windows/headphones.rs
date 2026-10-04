//! The default output on Windows (TUR-65): the render endpoint's form
//! factor (`PKEY_AudioEndpoint_FormFactor`), its name, and the bus of the
//! adapter behind it. The bus is what tells a Bluetooth headset apart, since
//! Windows' Bluetooth driver never puts "Bluetooth" in the endpoint's name.
//! Property reads and a topology walk only: no audio client is opened.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    IDeviceTopology, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    PKEY_AudioEndpoint_FormFactor, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PropVariantToStringAlloc, PropVariantToUInt32,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::core::PWSTR;

use super::activity::ComGuard;
use crate::Error;
use crate::headphones::OutputDevice;
use crate::platform::headphones::parse::{adapter_enumerator, windows_form, windows_transport};

/// The default render endpoint (the console role, what apps play to), or
/// `None` when there is none.
pub(crate) fn default_output_info() -> Result<Option<OutputDevice>, Error> {
    let _com = ComGuard::initialize();
    // SAFETY: COM is initialised on this thread for the guard's lifetime.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(read_error)?;
    // SAFETY: a valid enumerator; no default endpoint is an error, not UB.
    let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }) else {
        return Ok(None);
    };
    let name = string_property(&device, &PKEY_Device_FriendlyName).unwrap_or_default();
    let form_factor = u32_property(&device, &PKEY_AudioEndpoint_FormFactor);
    let adapter = adapter_device_id(&device);
    Ok(Some(OutputDevice {
        transport: windows_transport(adapter.as_deref().and_then(adapter_enumerator), &name),
        form: windows_form(form_factor),
        name,
    }))
}

fn read_error(error: windows::core::Error) -> Error {
    Error::DeviceRead(error.to_string())
}

/// The id of the adapter device the endpoint's first connector leads to, a
/// device path such as `{2}.\\?\bthhfenum#bthhfpaudio#...`. `None` for an
/// endpoint with a software connection.
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
fn adapter_device_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: COM calls on a valid endpoint; the string is ours to free.
    unsafe {
        let topology: IDeviceTopology = device.Activate(CLSCTX_ALL, None).ok()?;
        let connector = topology.GetConnector(0).ok()?;
        take_string(connector.GetDeviceIdConnectedTo().ok()?)
    }
}

/// A COM-allocated string, freed after it is copied.
///
/// # Safety
/// `raw` must be null or a string from `CoTaskMemAlloc`.
unsafe fn take_string(raw: PWSTR) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    // SAFETY: a valid, NUL-terminated wide string until freed below.
    let text = unsafe { raw.to_string() }.ok();
    // SAFETY: COM allocated it and nothing else holds it.
    unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
    text
}

fn string_property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: a valid endpoint; the PROPVARIANT is cleared before returning.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(key).ok()?;
        let text = PropVariantToStringAlloc(&value)
            .ok()
            .and_then(|raw| take_string(raw));
        let _ = PropVariantClear(&mut value);
        text
    }
}

fn u32_property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<u32> {
    // SAFETY: as for `string_property`.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(key).ok()?;
        let number = PropVariantToUInt32(&value).ok();
        let _ = PropVariantClear(&mut value);
        number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_output_reads_or_fails_without_panicking() {
        // A CI runner may have no audio endpoint; either answer is fine.
        let _ = default_output_info();
    }
}
