//! Windows: `PBT_APMSUSPEND` (TUR-145).
//!
//! `PBT_APMSUSPEND` is what `WM_POWERBROADCAST` carries when the PC is about
//! to suspend. Top-level windows get that message, but meet-ai may have none
//! open (it lives in the tray), and a message-only window gets no
//! broadcasts. `RegisterSuspendResumeNotification` with
//! `DEVICE_NOTIFY_CALLBACK` delivers the same event to a callback instead,
//! no window needed. Microsoft gives an app "approximately two seconds" to
//! handle it, so the stop runs on its own thread and the callback waits
//! [`BUDGET`] for it; what is left (the transcript, usually) finishes after
//! the PC wakes. The audio is checkpointed every five seconds regardless.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, RegisterSuspendResumeNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMSUSPEND};

use super::{Handler, run_within};

/// How long the callback waits for the stop, under Windows' two seconds.
const BUDGET: Duration = Duration::from_millis(1_500);

/// The handler the callback runs. One per process: the hook is set up once.
static HANDLER: OnceLock<Handler> = OnceLock::new();

pub(super) fn on_will_sleep(handler: Handler) -> Result<(), String> {
    HANDLER
        .set(handler)
        .map_err(|_| "the sleep hook is already set up".to_string())?;
    // Windows reads the parameters through the pointer for as long as the
    // registration lasts, which is the life of the app: leaked on purpose.
    let parameters: &'static mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
        Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power_event),
            Context: std::ptr::null_mut(),
        }));
    let recipient = HANDLE(std::ptr::from_mut(parameters).cast::<c_void>());
    // SAFETY: with `DEVICE_NOTIFY_CALLBACK` the recipient is a pointer to a
    // `DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS`, which lives forever (above). The
    // registration handle is never unregistered, so it is dropped here.
    unsafe { RegisterSuspendResumeNotification(recipient, DEVICE_NOTIFY_CALLBACK) }
        .map(drop)
        .map_err(|error| format!("could not register for suspend events: {error}"))
}

/// Windows' power callback. Only a suspend matters; every event answers
/// `ERROR_SUCCESS` (0).
unsafe extern "system" fn on_power_event(
    _context: *const c_void,
    kind: u32,
    _setting: *const c_void,
) -> u32 {
    if kind == PBT_APMSUSPEND
        && let Some(handler) = HANDLER.get()
    {
        tracing::info!("the PC is going to sleep");
        if !run_within(handler, BUDGET) {
            tracing::warn!("the sleep stop is still running; it finishes after the wake");
        }
    }
    0
}
