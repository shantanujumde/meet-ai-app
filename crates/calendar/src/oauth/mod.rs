//! Sign in with Google or Microsoft: OAuth 2 authorization code + PKCE (S256)
//! over a loopback redirect, the same on macOS, Windows and Linux (SPEC §2.7,
//! A12).
//!
//! The flow, split so the provider-neutral half is tested without a browser,
//! a network or a keychain:
//!
//! 1. [`CalendarAuth::begin_sign_in`] makes the PKCE verifier and a 32-byte
//!    random `state`, and builds the provider's authorization URL for a
//!    redirect `http://127.0.0.1:<port>/callback`.
//! 2. The app (src-tauri) listens on that port, opens the browser, and waits
//!    for the callback URL.
//! 3. [`CalendarAuth::finish_sign_in`] checks `state` (any local process can
//!    hit the loopback port, so a reply without our `state` is rejected and
//!    nothing is stored), exchanges the code, and keeps the refresh token in
//!    the OS keystore ([`TokenStore`]). The access token stays in memory.
//! 4. [`CalendarAuth::access_token`] is what the Google and Microsoft calendar
//!    providers call before each read. It refreshes silently, and answers
//!    [`Error::SignInExpired`] when the refresh token is gone or rejected.
//!
//! No HTTP client lives in this crate: the app passes one in ([`HttpClient`]).
//! That keeps `crates/calendar` free of a network stack (and of ring's C
//! build for the Windows cross-check), and lets the tests answer for Google.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use oauth2::basic::{BasicErrorResponseType, BasicTokenType};
use oauth2::{
    AuthorizationCode, CsrfToken, ExtraTokenFields, HttpRequest, HttpResponse, PkceCodeChallenge,
    PkceCodeVerifier, RefreshToken, RequestTokenError, Scope, StandardTokenResponse,
    TokenResponse as _,
};

use crate::Error;

mod callback;
mod providers;
mod token_store;
mod wire;

pub use callback::{account_label, carries_state, check_callback};
pub use providers::{Endpoints, ProviderId};
#[cfg(any(test, feature = "fake"))]
pub use token_store::MemoryStore;
pub use token_store::{KEYRING_SERVICE, KeyringStore, StoreError, TokenStore, keyring_user};
use wire::{access_of, build_client, exchange_error, request_error_detail};

/// How long a sign-in waits for the browser before giving up.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The path of the loopback redirect: `http://127.0.0.1:<port>/callback`.
pub const CALLBACK_PATH: &str = "/callback";

/// Random bytes in `state`, before base64url.
const STATE_BYTES: u32 = 32;

/// Refresh this long before the access token runs out, so a read that starts
/// just before expiry does not fail halfway.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// One provider's OAuth client, from `config.jsonc` (SPEC §8.1).
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub client_id: String,
    /// Google desktop clients get a client secret that Google itself calls
    /// non-secret; it must still be sent. Microsoft public clients have none.
    pub client_secret: Option<String>,
}

impl std::fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthClient")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "…"))
            .finish()
    }
}

/// An HTTP request that could not be sent or answered.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

/// The one way this module reaches the network: the token endpoint. The app
/// passes a real client; tests pass a fake that answers for the provider.
pub trait HttpClient: Send + Sync {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Everything that can go wrong signing in or out.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    /// No client id in `config.jsonc`.
    #[error("{} sign-in is not set up: add {key} to config.jsonc", provider.display_name())]
    NotConfigured {
        provider: ProviderId,
        key: &'static str,
    },
    /// The user said no, closed the browser, or never came back in time.
    #[error("{} sign-in was cancelled or timed out", provider.display_name())]
    Cancelled { provider: ProviderId },
    /// The reply was wrong (a `state` mismatch, no code, a refused exchange).
    /// Nothing was stored.
    #[error("{} sign-in failed: {detail}", provider.display_name())]
    Failed {
        provider: ProviderId,
        detail: String,
    },
    /// The token endpoint could not be reached.
    #[error("could not reach {}: {detail}", provider.display_name())]
    Unreachable {
        provider: ProviderId,
        detail: String,
    },
    /// The OS keystore refused to forget the token (sign-out).
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Whether a provider is signed in, as the settings screen shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    SignedIn,
    /// A refresh token is stored but the provider rejected it: sign in again.
    Expired,
    SignedOut,
}

/// One provider's sign-in, for the settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub provider: ProviderId,
    /// The email address (Google) or username (Microsoft) from the id_token.
    /// Display only. `None` when not known yet, say offline after a restart.
    pub label: Option<String>,
    pub state: AccountState,
    /// `false` when the OS keystore refused the token (Linux with no Secret
    /// Service): the sign-in lasts until the app quits.
    pub remembered: bool,
}

/// A sign-in waiting for the browser: made by
/// [`CalendarAuth::begin_sign_in`], finished by
/// [`CalendarAuth::finish_sign_in`].
pub struct PendingSignIn {
    provider: ProviderId,
    client: OAuthClient,
    redirect_uri: String,
    state: CsrfToken,
    verifier: PkceCodeVerifier,
    authorize_url: String,
}

impl PendingSignIn {
    pub fn provider(&self) -> ProviderId {
        self.provider
    }

    /// The page to open in the browser.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// The `state` the reply must carry, for the loopback listener's
    /// [`carries_state`] check.
    pub fn state(&self) -> &str {
        self.state.secret()
    }
}

/// The id_token next to the access token: only read for the account label.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct IdTokenFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}

impl ExtraTokenFields for IdTokenFields {}

type TokenResponse = StandardTokenResponse<IdTokenFields, BasicTokenType>;

/// Where the OAuth clients come from. The app reads `config.jsonc` on each
/// call, so pasting a client id needs no restart.
pub type ClientSource = Box<dyn Fn(ProviderId) -> Option<OAuthClient> + Send + Sync>;

/// What is known about one provider in this run of the app.
#[derive(Default)]
struct Session {
    /// The access token and when it runs out. Memory only, never stored.
    access: Option<(String, Instant)>,
    /// The refresh token, when the keystore refused it: lasts until quit.
    unsaved_refresh: Option<String>,
    label: Option<String>,
    /// The provider rejected the stored refresh token.
    expired: bool,
    /// The client id that was rejected, so a corrected `config.jsonc` gets
    /// one more try without a restart (TUR-88).
    expired_client: Option<String>,
}

/// Sign-in state for both cloud calendars. One per app, shared.
pub struct CalendarAuth {
    clients: ClientSource,
    store: Box<dyn TokenStore>,
    http: Box<dyn HttpClient>,
    /// One lock per provider, held across a refresh so two reads never
    /// refresh at once (Microsoft rotates refresh tokens on every use).
    sessions: [Mutex<Session>; 2],
}

impl CalendarAuth {
    pub fn new(
        clients: ClientSource,
        store: Box<dyn TokenStore>,
        http: Box<dyn HttpClient>,
    ) -> Self {
        Self {
            clients,
            store,
            http,
            sessions: Default::default(),
        }
    }

    fn session(&self, provider: ProviderId) -> MutexGuard<'_, Session> {
        let index = match provider {
            ProviderId::Google => 0,
            ProviderId::Microsoft => 1,
        };
        self.sessions[index]
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn client(&self, provider: ProviderId) -> Option<OAuthClient> {
        (self.clients)(provider).filter(|client| !client.client_id.trim().is_empty())
    }

    /// Start a sign-in: the PKCE pair, `state`, and the URL to open.
    /// `redirect_uri` is the loopback listener's, see [`redirect_uri`].
    pub fn begin_sign_in(
        &self,
        provider: ProviderId,
        redirect_uri: String,
    ) -> Result<PendingSignIn, SignInError> {
        let client = self.client(provider).ok_or(SignInError::NotConfigured {
            provider,
            key: provider.client_id_key(),
        })?;
        let oauth = build_client(provider, &client, Some(&redirect_uri))?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = CsrfToken::new_random_len(STATE_BYTES);
        let endpoints = provider.endpoints();
        let mut request = oauth
            .authorize_url(|| state.clone())
            .add_scopes(endpoints.scopes.iter().map(|s| Scope::new((*s).to_owned())))
            .set_pkce_challenge(challenge);
        for (name, value) in endpoints.extra_auth_params {
            request = request.add_extra_param(*name, *value);
        }
        let (url, _) = request.url();
        Ok(PendingSignIn {
            provider,
            client,
            redirect_uri,
            state,
            verifier,
            authorize_url: url.to_string(),
        })
    }

    /// Finish a sign-in with the URL the browser was sent back to.
    ///
    /// A `state` that is missing or not ours is [`SignInError::Failed`], and
    /// nothing is exchanged or stored.
    pub fn finish_sign_in(
        &self,
        pending: PendingSignIn,
        callback_url: &str,
    ) -> Result<Account, SignInError> {
        let provider = pending.provider;
        let code = check_callback(provider, callback_url, pending.state.secret())?;
        let oauth = build_client(provider, &pending.client, Some(&pending.redirect_uri))?;
        let http = |request| self.http.execute(request);
        let response = oauth
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pending.verifier)
            .request(&http)
            .map_err(|error| exchange_error(provider, error))?;
        let refresh = response
            .refresh_token()
            .map(|token| token.secret().clone())
            .ok_or_else(|| SignInError::Failed {
                provider,
                detail: "the reply had no refresh token".into(),
            })?;

        let mut session = self.session(provider);
        *session = Session::default();
        let remembered = self.keep_refresh_token(provider, &mut session, &refresh);
        session.label = account_label(provider, response.extra_fields().id_token.as_deref());
        session.access = Some(access_of(&response));
        tracing::info!(%provider, remembered, "signed in to a calendar");
        Ok(Account {
            provider,
            label: session.label.clone(),
            state: AccountState::SignedIn,
            remembered,
        })
    }

    /// Store `refresh` in the keystore, or keep it in memory when the
    /// keystore refuses it. Returns whether it was stored.
    fn keep_refresh_token(
        &self,
        provider: ProviderId,
        session: &mut Session,
        refresh: &str,
    ) -> bool {
        match self.store.save(provider, refresh) {
            Ok(()) => {
                session.unsaved_refresh = None;
                true
            }
            Err(error) => {
                tracing::warn!(
                    %provider,
                    %error,
                    "sign-in won't be remembered: no Secret Service (gnome-keyring or KWallet) running, or the OS keystore refused the token"
                );
                session.unsaved_refresh = Some(refresh.to_owned());
                false
            }
        }
    }

    /// Forget a provider: the access token, and the stored refresh token.
    pub fn sign_out(&self, provider: ProviderId) -> Result<(), SignInError> {
        let mut session = self.session(provider);
        let only_in_memory = session.unsaved_refresh.is_some();
        *session = Session::default();
        match self.store.delete(provider) {
            Ok(()) => Ok(()),
            // The keystore never had it, so there is nothing left to forget.
            Err(_) if only_in_memory => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Both providers' sign-in state.
    ///
    /// After a restart only the refresh token is known, so the first call
    /// refreshes once (network) to learn the account and whether the token
    /// still works. Offline, a stored token counts as signed in.
    pub fn accounts(&self) -> Vec<Account> {
        ProviderId::ALL
            .into_iter()
            .map(|provider| self.account(provider))
            .collect()
    }

    fn account(&self, provider: ProviderId) -> Account {
        let mut session = self.session(provider);
        let remembered = session.unsaved_refresh.is_none();
        let stored = session.unsaved_refresh.is_some() || session.access.is_some() || {
            match self.store.load(provider) {
                Ok(token) => token.is_some(),
                Err(error) => {
                    tracing::debug!(%provider, %error, "could not read the OS keystore");
                    false
                }
            }
        };
        let state = if !stored {
            session.expired = false;
            AccountState::SignedOut
        } else if self.still_expired(provider, &mut session) {
            AccountState::Expired
        } else if session.access.is_some() {
            AccountState::SignedIn
        } else {
            match self.refresh(provider, &mut session) {
                Err(Error::SignInExpired { .. }) if session.expired => AccountState::Expired,
                _ => AccountState::SignedIn,
            }
        };
        Account {
            provider,
            label: session.label.clone(),
            state,
            remembered,
        }
    }

    /// A valid access token for `provider`, refreshed silently when it is
    /// about to run out. Blocking: may call the token endpoint.
    ///
    /// [`Error::SignInExpired`] when there is no refresh token or the
    /// provider rejected it; [`Error::Unreachable`] when the token endpoint
    /// could not be reached.
    pub fn access_token(&self, provider: ProviderId) -> Result<String, Error> {
        let mut session = self.session(provider);
        if let Some((token, expires)) = &session.access
            && Instant::now() + EXPIRY_MARGIN < *expires
        {
            return Ok(token.clone());
        }
        self.refresh(provider, &mut session)
    }

    /// The calendar API answered 401 with `rejected`: forget it, so the next
    /// [`Self::access_token`] refreshes instead of handing it out again
    /// (TUR-47). A token that was already replaced is left alone, so two
    /// reads that both saw a 401 refresh only once.
    pub fn reject_access_token(&self, provider: ProviderId, rejected: &str) {
        let mut session = self.session(provider);
        if session
            .access
            .as_ref()
            .is_some_and(|(token, _)| token == rejected)
        {
            session.access = None;
        }
    }

    /// The calendar API answered 401 with `rejected`: a fresh access token,
    /// refreshed rather than cached. Both cloud providers' retry-once calls
    /// this (Microsoft TUR-47, Google TUR-48), so the renewal is written once.
    /// [`Error::SignInExpired`] when the refresh is refused.
    pub fn renew_access_token(
        &self,
        provider: ProviderId,
        rejected: &str,
    ) -> Result<String, Error> {
        self.reject_access_token(provider, rejected);
        self.access_token(provider)
    }

    /// Whether `provider` has a sign-in at all, working or expired: a
    /// refresh token in the keystore or in memory. No network, so the app can
    /// ask before every read which calendars to read (TUR-47).
    pub fn has_sign_in(&self, provider: ProviderId) -> bool {
        let session = self.session(provider);
        if session.unsaved_refresh.is_some() || session.access.is_some() {
            return true;
        }
        match self.store.load(provider) {
            Ok(token) => token.is_some(),
            // TUR-88: a locked or refusing keystore is not "signed out". Say
            // yes, so the read goes ahead and reports the keystore as
            // unreachable ([`Self::access_token`]) instead of the calendar
            // quietly vanishing.
            Err(error) => {
                tracing::debug!(%provider, %error, "could not read the OS keystore");
                true
            }
        }
    }

    /// `session.expired`, cleared first when the rejection was for another
    /// client id than the one configured now: the user fixed a mistyped
    /// `client_id`, and pasting one needs no restart (TUR-88).
    fn still_expired(&self, provider: ProviderId, session: &mut Session) -> bool {
        if session.expired {
            let now = self.client(provider).map(|c| c.client_id.trim().to_owned());
            if session.expired_client.is_some() && session.expired_client != now {
                tracing::info!(%provider, "the calendar client id changed; trying the sign-in again");
                session.expired = false;
                session.expired_client = None;
            }
        }
        session.expired
    }

    fn refresh(&self, provider: ProviderId, session: &mut Session) -> Result<String, Error> {
        let expired = Error::SignInExpired {
            provider: provider.display_name(),
        };
        if self.still_expired(provider, session) {
            return Err(expired);
        }
        let unreachable = |detail: String| Error::Unreachable {
            provider: provider.display_name(),
            detail,
        };
        let stored = match &session.unsaved_refresh {
            Some(token) => Some(token.clone()),
            // TUR-88: a keystore that cannot be read says nothing about the
            // sign-in, so it is not `SignInExpired` ("sign in again").
            None => self.store.load(provider).map_err(|error| {
                tracing::debug!(%provider, %error, "could not read the OS keystore");
                unreachable(error.to_string())
            })?,
        };
        let Some(refresh) = stored else {
            session.access = None;
            return Err(expired);
        };
        let Some(client) = self.client(provider) else {
            tracing::warn!(%provider, key = provider.client_id_key(), "cannot refresh a calendar sign-in: no client id in config.jsonc");
            return Err(expired);
        };
        let oauth = build_client(provider, &client, None)
            .map_err(|error| unreachable(error.to_string()))?;
        let http = |request| self.http.execute(request);
        let token = RefreshToken::new(refresh.clone());
        let response = match oauth.exchange_refresh_token(&token).request(&http) {
            Ok(response) => response,
            Err(RequestTokenError::ServerResponse(error))
                if matches!(
                    error.error(),
                    BasicErrorResponseType::InvalidGrant
                        | BasicErrorResponseType::InvalidClient
                        | BasicErrorResponseType::UnauthorizedClient
                ) =>
            {
                tracing::warn!(%provider, error = %error.error(), "the calendar sign-in was rejected; sign in again");
                session.access = None;
                session.expired = true;
                session.expired_client = Some(client.client_id.trim().to_owned());
                return Err(expired);
            }
            Err(error) => return Err(unreachable(request_error_detail(&error))),
        };
        // Microsoft hands out a new refresh token on every refresh. The old
        // one stays valid until it expires; keep the new one, whose lifetime
        // starts now.
        if let Some(rotated) = response.refresh_token()
            && rotated.secret() != &refresh
        {
            self.keep_refresh_token(provider, session, rotated.secret());
        }
        if let Some(label) = account_label(provider, response.extra_fields().id_token.as_deref()) {
            session.label = Some(label);
        }
        let access = access_of(&response);
        let secret = access.0.clone();
        session.access = Some(access);
        Ok(secret)
    }
}

#[cfg(test)]
mod tests;
