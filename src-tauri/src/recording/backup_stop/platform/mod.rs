//! The OS's "about to sleep" signal, for the sleep stop (TUR-145).
//!
//! The only place the backup stops name an OS (rule R10). [`on_will_sleep`]
//! registers `handler` to run when the computer is about to sleep or its lid
//! closes, for the life of the app:
//!
//! * macOS: `NSWorkspaceWillSleepNotification` (`macos.rs`).
//! * Windows: `PBT_APMSUSPEND`, the `WM_POWERBROADCAST` event, through a
//!   suspend/resume callback, which needs no window (`windows.rs`).
//! * Linux: logind's `PrepareForSleep(true)` signal, with a delay inhibitor
//!   so the stop gets its few seconds (`linux.rs`).
//! * Anything else: no signal; recordings are not stopped for sleep there.
//!
//! What the handler does (stop the recording) is OS-free and lives in
//! `backup_stop.rs`. [`run_within`] is the shared "give it this long, then
//! let the OS sleep" wait for the hooks that call it off the main thread.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// What runs when the computer is about to sleep.
pub type Handler = Arc<dyn Fn() + Send + Sync>;

/// Run `handler` every time the computer is about to sleep. `Err` says why
/// the hook could not be set up; the app runs on without it.
pub fn on_will_sleep(handler: Handler) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::on_will_sleep(handler)
    }
    #[cfg(windows)]
    {
        windows::on_will_sleep(handler)
    }
    #[cfg(target_os = "linux")]
    {
        linux::on_will_sleep(handler)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = handler;
        Err("this OS has no sleep signal meet-ai listens for".into())
    }
}

/// Run `handler` on a thread of its own and wait for it at most `budget`,
/// so the OS's own deadline is never missed by much. A handler that takes
/// longer goes on running; whatever it has not done by the time the
/// computer sleeps, it finishes after the wake.
#[cfg_attr(target_os = "macos", allow(dead_code, reason = "macOS runs it inline"))]
pub fn run_within(handler: &Handler, budget: Duration) -> bool {
    let (done, finished) = mpsc::channel();
    let handler = Arc::clone(handler);
    let spawned = std::thread::Builder::new()
        .name("meet-ai-sleep-stop".to_owned())
        .spawn(move || {
            handler();
            let _ = done.send(());
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not start the sleep stop");
        return false;
    }
    finished.recv_timeout(budget).is_ok()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn a_quick_handler_finishes_within_the_budget() {
        let ran = Arc::new(AtomicBool::new(false));
        let handler: Handler = {
            let ran = Arc::clone(&ran);
            Arc::new(move || ran.store(true, Ordering::SeqCst))
        };
        assert!(run_within(&handler, Duration::from_secs(5)));
        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn a_slow_handler_is_left_to_finish_on_its_own() {
        let handler: Handler = Arc::new(|| std::thread::sleep(Duration::from_millis(300)));
        assert!(!run_within(&handler, Duration::from_millis(10)));
    }
}
