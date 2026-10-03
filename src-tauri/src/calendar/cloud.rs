//! The cloud calendars (SPEC §2.7, A12): Microsoft (TUR-47) and Google
//! (TUR-48), read with the app's one [`CalendarAuth`].
//!
//! [`CalendarState`](super::CalendarState) picks providers on every read and
//! has no `AppHandle`, so [`init`] keeps one here at startup. A cloud
//! provider is only read when `calendar.providers` names it *and* it has a
//! sign-in ([`CalendarAuth::has_sign_in`], no network). An expired sign-in
//! still counts, so the read answers `SignInExpired` and the Today pane shows
//! its designed state; signed out, the provider is left out quietly.

use std::sync::OnceLock;

use ::calendar::CalendarProvider;
use ::calendar::Error;
use ::calendar::microsoft::{MicrosoftProvider, TokenSource};
use ::calendar::oauth::{CalendarAuth, ProviderId};
use tauri::{AppHandle, Manager as _};

use super::signin::ReqwestHttp;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Keep the app handle so cloud providers can reach [`CalendarAuth`]. Called
/// once from `setup`; a second call is ignored.
pub(crate) fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The cloud provider for `id`, when it has a sign-in. `None` before
/// [`init`], when signed out, and for a provider not built yet.
pub(crate) fn provider(id: ProviderId) -> Option<Box<dyn CalendarProvider + Send + Sync>> {
    let app = APP.get()?;
    if !app.try_state::<CalendarAuth>()?.has_sign_in(id) {
        tracing::debug!(provider = %id, "calendar provider is configured but not signed in; skipping it");
        return None;
    }
    match id {
        ProviderId::Microsoft => Some(Box::new(MicrosoftProvider::new(
            Box::new(AppTokens(app.clone())),
            Box::new(ReqwestHttp::default()),
        ))),
        // TUR-48.
        ProviderId::Google => None,
    }
}

/// The Microsoft access token from the app's managed [`CalendarAuth`].
struct AppTokens(AppHandle);

impl AppTokens {
    fn auth(&self) -> Result<tauri::State<'_, CalendarAuth>, Error> {
        self.0
            .try_state::<CalendarAuth>()
            .ok_or(Error::SignInExpired {
                provider: ProviderId::Microsoft.display_name(),
            })
    }
}

impl TokenSource for AppTokens {
    fn access_token(&self) -> Result<String, Error> {
        TokenSource::access_token(self.auth()?.inner())
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        self.auth()?.inner().renew_access_token(rejected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_read_before_init() {
        // Unit tests never run `setup`, so there is no app handle.
        assert!(provider(ProviderId::Microsoft).is_none());
        assert!(provider(ProviderId::Google).is_none());
    }
}
