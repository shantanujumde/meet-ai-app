//! Sign in with Google or Microsoft (TUR-44, SPEC §2.7, A12): the app half
//! of [`::calendar::oauth`].
//!
//! The provider-neutral flow (PKCE, `state`, the code exchange, the keystore)
//! lives in `crates/calendar/src/oauth/`. This file adds what needs the app:
//! the loopback listener (`super::loopback`, TUR-88), the browser
//! (the opener plugin), the one HTTP client (`reqwest`, blocking, on the
//! blocking pool), the client ids from `config.jsonc`, and three commands for
//! the Settings card (TUR-49).
//!
//! The Google and Microsoft calendar providers (TUR-47, TUR-48) read
//! `app.state::<CalendarAuth>()` and call
//! [`CalendarAuth::access_token`](::calendar::oauth::CalendarAuth::access_token).

use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

use ::calendar::oauth::{
    self, Account, AccountState, CalendarAuth, HttpClient, HttpError, KeyringStore, OAuthClient,
    ProviderId, SignInError,
};
use oauth2::{HttpRequest, HttpResponse, SyncHttpClient as _};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _};

use crate::config;
use crate::error::UiError;

/// The page the browser shows after the redirect.
const CALLBACK_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>meet-ai</title></head><body><p>Signed in. You can close this tab and go back to meet-ai.</p></body></html>";

/// How long one token request may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// A cloud calendar that needs a sign-in, as the webview names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum SignInProvider {
    Google,
    Microsoft,
}

impl SignInProvider {
    /// Every sign-in, in the order the Settings card lists them.
    pub const ALL: [Self; 2] = [Self::Google, Self::Microsoft];
}

impl From<SignInProvider> for ProviderId {
    fn from(provider: SignInProvider) -> Self {
        match provider {
            SignInProvider::Google => Self::Google,
            SignInProvider::Microsoft => Self::Microsoft,
        }
    }
}

impl From<ProviderId> for SignInProvider {
    fn from(provider: ProviderId) -> Self {
        match provider {
            ProviderId::Google => Self::Google,
            ProviderId::Microsoft => Self::Microsoft,
        }
    }
}

/// Whether a provider is signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum SignInState {
    SignedIn,
    /// The provider rejected the stored sign-in: sign in again.
    Expired,
    SignedOut,
}

/// One provider's sign-in, for the Settings card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAccount {
    pub provider: SignInProvider,
    /// The email address or username. Display only; `null` when not known
    /// yet (offline right after a restart).
    pub account: Option<String>,
    pub state: SignInState,
    /// `false`: the OS keystore refused the token (Linux with no Secret
    /// Service), so this sign-in lasts until the app quits.
    pub remembered: bool,
}

impl From<Account> for CalendarAccount {
    fn from(account: Account) -> Self {
        Self {
            provider: account.provider.into(),
            account: account.label,
            state: match account.state {
                AccountState::SignedIn => SignInState::SignedIn,
                AccountState::Expired => SignInState::Expired,
                AccountState::SignedOut => SignInState::SignedOut,
            },
            remembered: account.remembered,
        }
    }
}

impl From<SignInError> for UiError {
    fn from(error: SignInError) -> Self {
        let kind = match &error {
            SignInError::NotConfigured { .. } => "calendar-not-configured",
            SignInError::Cancelled { .. } => "calendar-sign-in-cancelled",
            SignInError::Failed { .. } => "calendar-sign-in-failed",
            SignInError::Unreachable { .. } => "calendar-unreachable",
            SignInError::Store(_) => "calendar-keystore",
        };
        Self::app(kind, error.to_string())
    }
}

/// The app's [`CalendarAuth`]: client ids from `config.jsonc` (read on each
/// use), the OS keystore, and reqwest.
pub fn auth() -> CalendarAuth {
    CalendarAuth::new(
        Box::new(configured_client),
        Box::new(KeyringStore),
        Box::new(ReqwestHttp::default()),
    )
}

/// `calendar.google` / `calendar.microsoft` from `config.jsonc`.
fn configured_client(provider: ProviderId) -> Option<OAuthClient> {
    let calendar = config::calendar();
    let client = match provider {
        ProviderId::Google => calendar.google,
        ProviderId::Microsoft => calendar.microsoft,
    };
    Some(OAuthClient {
        client_id: client.client_id?,
        client_secret: client.client_secret,
    })
}

/// The token endpoint over reqwest's blocking client.
///
/// Made on first use, on a blocking-pool thread: reqwest's blocking client
/// runs its own runtime and must not be built inside an async one.
#[derive(Default)]
pub(crate) struct ReqwestHttp {
    client: OnceLock<Result<reqwest::blocking::Client, String>>,
}

impl HttpClient for ReqwestHttp {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let client = self
            .client
            .get_or_init(|| {
                reqwest::blocking::Client::builder()
                    // oauth2's docs: never follow redirects from a token
                    // endpoint (SSRF).
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(HTTP_TIMEOUT)
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|error| HttpError(error.clone()))?;
        client
            .call(request)
            .map_err(|error| HttpError(error.to_string()))
    }
}

/// Sign in: listen on a random loopback port, open the browser with
/// `open`, wait up to `timeout` for the redirect, then finish the flow.
///
/// Blocking. A timeout, or a listener that stops without a reply, is
/// [`SignInError::Cancelled`]. The browser cannot tell us it was closed, so
/// a closed tab ends as the timeout.
pub fn sign_in_blocking(
    auth: &CalendarAuth,
    provider: ProviderId,
    open: impl FnOnce(&str) -> Result<(), String>,
    timeout: Duration,
) -> Result<Account, SignInError> {
    // Stopped when it goes out of scope, on every path.
    let listener = super::loopback::listen(CALLBACK_PAGE).map_err(|error| SignInError::Failed {
        provider,
        detail: format!("could not listen on 127.0.0.1: {error}"),
    })?;
    let pending = auth.begin_sign_in(provider, oauth::redirect_uri(listener.port()))?;
    if let Err(detail) = open(pending.authorize_url()) {
        return Err(SignInError::Failed {
            provider,
            detail: format!("could not open the browser: {detail}"),
        });
    }
    match listener.next_callback(timeout) {
        Ok(url) => auth.finish_sign_in(pending, &url),
        Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
            Err(SignInError::Cancelled { provider })
        }
    }
}

/// Run `work` with the app's [`CalendarAuth`] on the blocking pool: every
/// call here may wait on the network or the keystore.
async fn on_blocking_pool<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle, &CalendarAuth) -> T + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(move || work(&app, &app.state::<CalendarAuth>()))
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))
}

/// Sign in to Google or Microsoft in the browser, and return the account.
///
/// Errors: `calendar-not-configured` (no client id; the message names the
/// `config.jsonc` key), `calendar-sign-in-cancelled` (said no, or no reply in
/// 5 minutes), `calendar-sign-in-failed` (a bad reply, such as a `state`
/// mismatch; nothing is stored), `calendar-unreachable`.
#[tauri::command]
#[specta::specta]
pub async fn calendar_sign_in(
    app: AppHandle,
    provider: SignInProvider,
) -> Result<CalendarAccount, UiError> {
    on_blocking_pool(app, move |app, auth| {
        use tauri_plugin_opener::OpenerExt as _;
        let open = |url: &str| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|error| error.to_string())
        };
        sign_in_blocking(auth, provider.into(), open, oauth::SIGN_IN_TIMEOUT)
            .map(CalendarAccount::from)
            .map_err(UiError::from)
    })
    .await?
}

/// Sign out: forget the access token and delete the stored refresh token.
#[tauri::command]
#[specta::specta]
pub async fn calendar_sign_out(app: AppHandle, provider: SignInProvider) -> Result<(), UiError> {
    on_blocking_pool(app, move |_, auth| {
        auth.sign_out(provider.into()).map_err(UiError::from)
    })
    .await?
}

/// Both providers' sign-in state. The first call after a launch refreshes
/// each stored sign-in once, to learn the account and whether it still works.
#[tauri::command]
#[specta::specta]
pub async fn calendar_accounts(app: AppHandle) -> Result<Vec<CalendarAccount>, UiError> {
    on_blocking_pool(app, |_, auth| {
        auth.accounts()
            .into_iter()
            .map(CalendarAccount::from)
            .collect()
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::net::TcpStream;
    use std::sync::Arc;

    use ::calendar::oauth::{MemoryStore, StoreError, TokenStore};
    use oauth2::url::Url;

    use super::*;

    /// Answers every token request with one fixed token.
    struct FakeTokenEndpoint;

    impl HttpClient for FakeTokenEndpoint {
        fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, HttpError> {
            let body = serde_json::json!({
                "access_token": "access",
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "refresh",
            });
            Ok(oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(body.to_string().into_bytes())
                .unwrap())
        }
    }

    struct SharedStore(Arc<MemoryStore>);

    impl TokenStore for SharedStore {
        fn load(&self, provider: ProviderId) -> Result<Option<String>, StoreError> {
            self.0.load(provider)
        }
        fn save(&self, provider: ProviderId, token: &str) -> Result<(), StoreError> {
            self.0.save(provider, token)
        }
        fn delete(&self, provider: ProviderId) -> Result<(), StoreError> {
            self.0.delete(provider)
        }
    }

    fn test_auth(configured: bool) -> (CalendarAuth, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        let auth = CalendarAuth::new(
            Box::new(move |_| {
                configured.then(|| OAuthClient {
                    client_id: "test-client".into(),
                    client_secret: None,
                })
            }),
            Box::new(SharedStore(store.clone())),
            Box::new(FakeTokenEndpoint),
        );
        (auth, store)
    }

    fn param(url: &str, name: &str) -> String {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap()
    }

    /// What the browser does after the provider: ask for the redirect URI
    /// with the reply in the query. Any local process can do this.
    fn send_to_listener(authorize_url: &str, callback_query: impl Fn(&str) -> String) {
        let redirect = param(authorize_url, "redirect_uri");
        let port = Url::parse(&redirect).unwrap().port().unwrap();
        let path = Url::parse(&redirect).unwrap().path().to_owned();
        let query = callback_query(&param(authorize_url, "state"));
        let request = format!("GET {path}?{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
        });
    }

    const WAIT: Duration = Duration::from_secs(10);

    #[test]
    fn the_browser_coming_back_with_our_state_signs_in_and_stores_the_refresh_token() {
        let (auth, store) = test_auth(true);
        let account = sign_in_blocking(
            &auth,
            ProviderId::Microsoft,
            |url| {
                assert!(param(url, "redirect_uri").starts_with("http://127.0.0.1:"));
                assert!(param(url, "redirect_uri").ends_with("/callback"));
                send_to_listener(url, |state| format!("state={state}&code=the-code"));
                Ok(())
            },
            WAIT,
        )
        .unwrap();
        assert_eq!(account.state, AccountState::SignedIn);
        assert!(account.remembered);
        assert_eq!(
            store.stored(ProviderId::Microsoft).as_deref(),
            Some("refresh")
        );
    }

    #[test]
    fn a_local_process_with_the_wrong_state_is_rejected_and_nothing_stored() {
        let (auth, store) = test_auth(true);
        let error = sign_in_blocking(
            &auth,
            ProviderId::Google,
            |url| {
                send_to_listener(url, |_| "state=forged&code=evil".into());
                Ok(())
            },
            WAIT,
        )
        .unwrap_err();
        assert!(matches!(error, SignInError::Failed { .. }), "{error:?}");
        assert_eq!(UiError::from(error).kind, "calendar-sign-in-failed");
        assert_eq!(store.stored(ProviderId::Google), None);
    }

    #[test]
    fn no_reply_in_time_is_cancelled() {
        let (auth, store) = test_auth(true);
        let error = sign_in_blocking(
            &auth,
            ProviderId::Google,
            |_| Ok(()),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(matches!(error, SignInError::Cancelled { .. }), "{error:?}");
        assert_eq!(UiError::from(error).kind, "calendar-sign-in-cancelled");
        assert_eq!(store.stored(ProviderId::Google), None);
    }

    #[test]
    fn no_client_id_is_not_configured_and_opens_no_browser() {
        let (auth, _) = test_auth(false);
        let error = sign_in_blocking(
            &auth,
            ProviderId::Google,
            |_| panic!("opened the browser without a client id"),
            WAIT,
        )
        .unwrap_err();
        let ui = UiError::from(error);
        assert_eq!(ui.kind, "calendar-not-configured");
        assert!(
            ui.message.contains("calendar.google.client_id"),
            "{}",
            ui.message
        );
    }

    #[test]
    fn a_browser_that_cannot_open_fails_the_sign_in() {
        let (auth, _) = test_auth(true);
        let error = sign_in_blocking(
            &auth,
            ProviderId::Google,
            |_| Err("no browser".into()),
            WAIT,
        )
        .unwrap_err();
        assert!(
            matches!(&error, SignInError::Failed { detail, .. } if detail.contains("no browser"))
        );
    }

    #[test]
    fn accounts_reach_the_webview_in_the_documented_shape() {
        let account = CalendarAccount::from(Account {
            provider: ProviderId::Google,
            label: Some("ada@example.com".into()),
            state: AccountState::SignedIn,
            remembered: false,
        });
        assert_eq!(
            serde_json::to_value(&account).unwrap(),
            serde_json::json!({
                "provider": "google",
                "account": "ada@example.com",
                "state": "signed-in",
                "remembered": false,
            })
        );
        let expired = serde_json::to_value(SignInState::Expired).unwrap();
        assert_eq!(expired, "expired");
        assert_eq!(
            serde_json::to_value(SignInState::SignedOut).unwrap(),
            "signed-out"
        );
        let provider: SignInProvider = serde_json::from_str("\"microsoft\"").unwrap();
        assert_eq!(ProviderId::from(provider), ProviderId::Microsoft);
    }

    #[test]
    fn the_callback_page_says_where_to_go() {
        assert!(
            CALLBACK_PAGE.contains("Signed in. You can close this tab and go back to meet-ai.")
        );
        // A static page: nothing from the request is echoed back into it.
        assert!(!CALLBACK_PAGE.contains("<script"));
    }
}
