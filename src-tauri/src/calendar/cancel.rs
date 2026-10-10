//! Cancel a sign-in that is waiting on the browser (TUR-174).
//!
//! The browser cannot tell the app its tab was closed, so a sign-in waits up
//! to [`::calendar::oauth::SIGN_IN_TIMEOUT`] for a redirect that will never
//! come. While it waits, its loopback's [`Canceller`] sits here under its
//! provider, and [`calendar_cancel_sign_in`] (the screen's Cancel button)
//! ends that wait at once, as `calendar-sign-in-cancelled`.

use std::sync::{Mutex, PoisonError};

use ::calendar::oauth::ProviderId;
use tauri::{AppHandle, Manager as _};

use super::loopback::Canceller;
use super::signin::SignInProvider;

/// Managed state: the sign-ins waiting on the browser, by provider.
#[derive(Default)]
pub struct SignInCancels {
    waiting: Mutex<Waiting>,
}

#[derive(Default)]
struct Waiting {
    /// Tells two sign-ins of one provider apart, so the first one ending does
    /// not forget the second.
    next_id: u64,
    entries: Vec<(ProviderId, u64, Canceller)>,
}

impl SignInCancels {
    /// Hold `canceller` for `provider` until the returned guard drops. A
    /// newer sign-in of the same provider replaces an older one here.
    pub fn register(&self, provider: ProviderId, canceller: Canceller) -> Registered<'_> {
        let mut waiting = self.lock();
        let id = waiting.next_id;
        waiting.next_id = waiting.next_id.wrapping_add(1);
        waiting.entries.retain(|(each, _, _)| *each != provider);
        waiting.entries.push((provider, id, canceller));
        Registered { cancels: self, id }
    }

    /// End `provider`'s waiting sign-in. `false` when none was waiting.
    pub fn cancel(&self, provider: ProviderId) -> bool {
        let found = {
            let mut waiting = self.lock();
            let at = waiting
                .entries
                .iter()
                .position(|(each, _, _)| *each == provider);
            at.map(|at| waiting.entries.remove(at).2)
        };
        match found {
            Some(canceller) => {
                canceller.cancel();
                true
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Waiting> {
        // Only plain data inside; a panic elsewhere left it consistent.
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A sign-in's place in [`SignInCancels`]; removed when it ends.
pub struct Registered<'a> {
    cancels: &'a SignInCancels,
    id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let id = self.id;
        self.cancels
            .lock()
            .entries
            .retain(|(_, each, _)| *each != id);
    }
}

/// Stop waiting for the browser: the sign-in for `provider` ends as
/// `calendar-sign-in-cancelled`, and nothing is stored. `false` when no
/// sign-in for it was waiting (it had already finished, say).
#[tauri::command]
#[specta::specta]
pub async fn calendar_cancel_sign_in(app: AppHandle, provider: SignInProvider) -> bool {
    app.state::<SignInCancels>().cancel(provider.into())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::loopback::{self, Ended};
    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    #[test]
    fn cancel_ends_only_that_providers_wait() {
        let cancels = SignInCancels::default();
        let google = loopback::listen("").unwrap();
        let microsoft = loopback::listen("").unwrap();
        let _g = cancels.register(ProviderId::Google, google.canceller());
        let _m = cancels.register(ProviderId::Microsoft, microsoft.canceller());
        assert!(cancels.cancel(ProviderId::Google));
        assert_eq!(google.next_callback(WAIT), Err(Ended::Cancelled));
        assert_eq!(
            microsoft.next_callback(Duration::from_millis(50)),
            Err(Ended::TimedOut)
        );
        // Already cancelled.
        assert!(!cancels.cancel(ProviderId::Google));
    }

    #[test]
    fn an_ended_sign_in_is_forgotten_and_a_newer_one_is_kept() {
        let cancels = SignInCancels::default();
        let first = loopback::listen("").unwrap();
        let second = loopback::listen("").unwrap();
        let older = cancels.register(ProviderId::Google, first.canceller());
        let _newer = cancels.register(ProviderId::Google, second.canceller());
        // The older one ending does not forget the newer one.
        drop(older);
        assert!(cancels.cancel(ProviderId::Google));
        assert_eq!(second.next_callback(WAIT), Err(Ended::Cancelled));

        let done = loopback::listen("").unwrap();
        drop(cancels.register(ProviderId::Microsoft, done.canceller()));
        assert!(!cancels.cancel(ProviderId::Microsoft));
    }
}
