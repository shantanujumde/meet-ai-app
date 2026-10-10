//! Running a capture build that can block on a TCC consent dialog on its
//! own thread, and giving up on it after a bound (TUR-162).
//!
//! The microphone's stream creation and the tap's `AudioHardwareCreateProcessTap`
//! block for as long as a person takes to answer the dialog, and forever when
//! nobody is there (see [`crate::AUDIO_PERMISSION_TIMEOUT`]). Neither can be
//! cancelled, so the caller waits on a channel with a timeout and leaks the
//! thread. What this module adds is the other end: when that thread finishes
//! after the caller has gone (the dialog answered late, say during
//! onboarding's system-audio check), its send fails and the built value is
//! dropped right there, on that thread. Each source's built value tears
//! itself down on drop, so a late build stops capturing at once instead of
//! running, unseen, until the app quits.

use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use crate::Error;

/// Run `build` on a new thread called `name` and wait up to `timeout` for
/// it. A timeout reads as [`Error::PermissionDenied`], as before: an
/// unanswered dialog is what blocks these calls. A build that finishes after
/// that is dropped on its own thread, which is what tears it down.
///
/// A thread that ends without an answer (`build` panicked: a 0 Hz device,
/// an Objective-C failure) is a device error, not a permission one (TUR-163):
/// it used to read as "permission denied", and the user was sent to System
/// Settings to turn on a permission that was already on.
pub(crate) fn run_bounded<T: Send + 'static>(
    name: &str,
    timeout: Duration,
    build: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            // Fails only when the caller gave up; the value it hands back
            // is dropped here, which stops whatever the build started.
            let _ = tx.send(build());
        })?;
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(Error::PermissionDenied),
        Err(RecvTimeoutError::Disconnected) => {
            tracing::warn!("{name} ended without an answer; its build panicked");
            Err(Error::DeviceRead(
                "opening the device stopped unexpectedly; the log has the details".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    /// Stands in for a source's built capture: records that it was dropped.
    struct Built(Arc<AtomicBool>);

    impl Drop for Built {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn a_build_in_time_is_handed_back_running() {
        let torn_down = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&torn_down);
        let built = run_bounded("test-init", Duration::from_secs(5), move || Ok(Built(flag)))
            .expect("the build finished in time");
        assert!(!torn_down.load(Ordering::Acquire));
        drop(built);
        assert!(torn_down.load(Ordering::Acquire));
    }

    /// The ticket's leak: a build that finishes after the caller gave up
    /// is torn down, not left running.
    #[test]
    fn a_build_that_finishes_after_the_timeout_is_torn_down() {
        let torn_down = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&torn_down);
        let result = run_bounded("test-init", Duration::from_millis(20), move || {
            std::thread::sleep(Duration::from_millis(200));
            Ok(Built(flag))
        });
        assert!(matches!(result, Err(Error::PermissionDenied)));

        let deadline = Instant::now() + Duration::from_secs(5);
        while !torn_down.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "the late build was never dropped"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// TUR-163's "Done when": a build that panics is a device error.
    #[test]
    fn a_build_that_panics_is_a_device_error_not_a_denial() {
        let result: Result<(), Error> = run_bounded("test-init", Duration::from_secs(5), || {
            panic!("a 0 Hz device");
        });
        assert!(matches!(result, Err(Error::DeviceRead(_))), "{result:?}");
    }

    #[test]
    fn a_failed_build_reports_its_own_error() {
        let result: Result<(), Error> = run_bounded("test-init", Duration::from_secs(5), || {
            Err(Error::NoDevice("none".into()))
        });
        assert!(matches!(result, Err(Error::NoDevice(_))));
    }
}
