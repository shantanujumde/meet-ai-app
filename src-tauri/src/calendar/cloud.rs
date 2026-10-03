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
use ::calendar::cloud::TokenSource;
use ::calendar::google::GoogleProvider;
use ::calendar::microsoft::MicrosoftProvider;
use ::calendar::oauth::{CalendarAuth, ProviderId};
use tauri::{AppHandle, Manager as _};

use super::signin::SharedHttp;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Keep the app handle so cloud providers can reach [`CalendarAuth`]. Called
/// once from `setup`; a second call is ignored.
pub(crate) fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The cloud provider for `id`, when it has a sign-in. `None` before
/// [`init`] and when signed out.
pub(crate) fn provider(id: ProviderId) -> Option<Box<dyn CalendarProvider + Send + Sync>> {
    let app = APP.get()?;
    if !app.try_state::<CalendarAuth>()?.has_sign_in(id) {
        tracing::debug!(provider = %id, "calendar provider is configured but not signed in; skipping it");
        return None;
    }
    let tokens = AppTokens {
        app: app.clone(),
        provider: id,
    };
    let http = Box::new(SharedHttp);
    match id {
        ProviderId::Microsoft => Some(Box::new(MicrosoftProvider::new(Box::new(tokens), http))),
        ProviderId::Google => Some(Box::new(GoogleProvider::new(Box::new(tokens), http))),
    }
}

/// One provider's access token from the app's managed [`CalendarAuth`].
struct AppTokens {
    app: AppHandle,
    provider: ProviderId,
}

impl AppTokens {
    fn auth(&self) -> Result<tauri::State<'_, CalendarAuth>, Error> {
        self.app
            .try_state::<CalendarAuth>()
            .ok_or(Error::SignInExpired {
                provider: self.provider.display_name(),
            })
    }
}

// Both providers take the one `TokenSource`; the renewal itself is
// `CalendarAuth::renew_access_token`.
impl TokenSource for AppTokens {
    fn access_token(&self) -> Result<String, Error> {
        self.auth()?.access_token(self.provider)
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        self.auth()?.renew_access_token(self.provider, rejected)
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
