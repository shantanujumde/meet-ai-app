//! The sign-in flow against a fake token endpoint and an in-memory keystore.
//! No browser, no network, no real keychain.

use std::collections::VecDeque;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oauth2::url::Url;

use super::*;

/// Answers token requests from a queue and remembers each request body.
#[derive(Default)]
struct FakeHttp {
    replies: Mutex<VecDeque<Result<(u16, String), String>>>,
    bodies: Mutex<Vec<String>>,
}

impl FakeHttp {
    fn reply(&self, status: u16, body: serde_json::Value) {
        self.replies
            .lock()
            .unwrap()
            .push_back(Ok((status, body.to_string())));
    }

    fn fail(&self, detail: &str) {
        self.replies.lock().unwrap().push_back(Err(detail.into()));
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }

    fn form(&self, index: usize) -> Vec<(String, String)> {
        let body = &self.bodies()[index];
        Url::parse(&format!("http://x/?{body}"))
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }
}

struct SharedHttp(Arc<FakeHttp>);

impl HttpClient for SharedHttp {
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let fake = &self.0;
        fake.bodies
            .lock()
            .unwrap()
            .push(String::from_utf8(request.body().clone()).unwrap());
        let reply = fake
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("an unexpected token request");
        let (status, body) = reply.map_err(HttpError)?;
        Ok(oauth2::http::Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(body.into_bytes())
            .unwrap())
    }
}

struct SharedStore(Arc<MemoryStore>);

impl TokenStore for SharedStore {
    fn load(&self, provider: ProviderId) -> Result<Option<String>, StoreError> {
        self.0.load(provider)
    }
    fn save(&self, provider: ProviderId, refresh_token: &str) -> Result<(), StoreError> {
        self.0.save(provider, refresh_token)
    }
    fn delete(&self, provider: ProviderId) -> Result<(), StoreError> {
        self.0.delete(provider)
    }
}

fn clients(provider: ProviderId) -> Option<OAuthClient> {
    Some(match provider {
        ProviderId::Google => OAuthClient {
            client_id: "google-id.apps.googleusercontent.com".into(),
            client_secret: Some("google-not-secret".into()),
        },
        ProviderId::Microsoft => OAuthClient {
            client_id: "microsoft-id".into(),
            client_secret: None,
        },
    })
}

fn auth_with(store: &Arc<MemoryStore>, http: &Arc<FakeHttp>) -> CalendarAuth {
    CalendarAuth::new(
        Box::new(clients),
        Box::new(SharedStore(store.clone())),
        Box::new(SharedHttp(http.clone())),
    )
}

fn setup() -> (CalendarAuth, Arc<MemoryStore>, Arc<FakeHttp>) {
    let store = Arc::new(MemoryStore::default());
    let http = Arc::new(FakeHttp::default());
    (auth_with(&store, &http), store, http)
}

fn id_token(claims: serde_json::Value) -> String {
    let encode = |value: serde_json::Value| URL_SAFE_NO_PAD.encode(value.to_string());
    format!(
        "{}.{}.not-checked",
        encode(serde_json::json!({ "alg": "RS256" })),
        encode(claims)
    )
}

fn token_reply(
    refresh: Option<&str>,
    expires_in: u64,
    claims: serde_json::Value,
) -> serde_json::Value {
    let mut reply = serde_json::json!({
        "access_token": "access-1",
        "token_type": "Bearer",
        "expires_in": expires_in,
        "id_token": id_token(claims),
    });
    if let Some(refresh) = refresh {
        reply["refresh_token"] = refresh.into();
    }
    reply
}

fn query(url: &str) -> Vec<(String, String)> {
    Url::parse(url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

fn get<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn callback(pending: &PendingSignIn, extra: &str) -> String {
    format!(
        "http://127.0.0.1:4711/callback?state={}&{extra}",
        pending.state.secret()
    )
}

/// Sign in to Google with a refresh token and an email in the id_token.
fn signed_in_google(auth: &CalendarAuth, http: &FakeHttp, expires_in: u64) -> Account {
    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    http.reply(
        200,
        token_reply(
            Some("refresh-1"),
            expires_in,
            serde_json::json!({ "email": "ada@example.com" }),
        ),
    );
    let url = callback(&pending, "code=the-code");
    auth.finish_sign_in(pending, &url).unwrap()
}

#[test]
fn the_authorize_url_uses_pkce_s256_a_32_byte_state_and_the_loopback_redirect() {
    let (auth, _, _) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    let url = pending.authorize_url();
    assert!(
        url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
        "{url}"
    );
    let pairs = query(url);
    assert_eq!(get(&pairs, "response_type"), Some("code"));
    assert_eq!(
        get(&pairs, "client_id"),
        Some("google-id.apps.googleusercontent.com")
    );
    assert_eq!(
        get(&pairs, "redirect_uri"),
        Some("http://127.0.0.1:4711/callback")
    );
    assert_eq!(get(&pairs, "code_challenge_method"), Some("S256"));
    assert_eq!(get(&pairs, "code_challenge").map(str::len), Some(43));
    assert_eq!(
        get(&pairs, "scope"),
        Some("openid email https://www.googleapis.com/auth/calendar.events.readonly")
    );
    assert_eq!(get(&pairs, "access_type"), Some("offline"));
    assert_eq!(get(&pairs, "prompt"), Some("consent"));
    // 32 bytes, base64url without padding.
    let state = get(&pairs, "state").unwrap();
    assert_eq!(URL_SAFE_NO_PAD.decode(state).unwrap().len(), 32);
    // The client secret never goes in the browser URL.
    assert!(!url.contains("google-not-secret"));
}

#[test]
fn microsoft_asks_for_offline_access_and_calendars_read() {
    let (auth, _, _) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Microsoft, redirect_uri(5000))
        .unwrap();
    let url = pending.authorize_url();
    assert!(
        url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"),
        "{url}"
    );
    let pairs = query(url);
    assert_eq!(
        get(&pairs, "scope"),
        Some("openid email offline_access https://graph.microsoft.com/Calendars.Read")
    );
    assert_eq!(get(&pairs, "prompt"), None);
}

#[test]
fn each_sign_in_gets_its_own_state() {
    let (auth, _, _) = setup();
    let a = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(1))
        .unwrap();
    let b = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(1))
        .unwrap();
    assert_ne!(a.state.secret(), b.state.secret());
}

#[test]
fn a_missing_client_id_names_the_config_key() {
    let auth = CalendarAuth::new(
        Box::new(|provider| match provider {
            ProviderId::Google => None,
            ProviderId::Microsoft => Some(OAuthClient {
                client_id: "  ".into(),
                client_secret: None,
            }),
        }),
        Box::new(MemoryStore::default()),
        Box::new(SharedHttp(Arc::default())),
    );
    for (provider, key) in [
        (ProviderId::Google, "calendar.google.client_id"),
        (ProviderId::Microsoft, "calendar.microsoft.client_id"),
    ] {
        let Err(error) = auth.begin_sign_in(provider, redirect_uri(1)) else {
            panic!("{provider} signed in without a client id");
        };
        assert!(matches!(error, SignInError::NotConfigured { key: k, .. } if k == key));
        assert!(error.to_string().contains(key), "{error}");
    }
}

#[test]
fn a_state_mismatch_is_rejected_and_nothing_is_exchanged_or_stored() {
    let (auth, store, http) = setup();
    for bad in [
        "http://127.0.0.1:4711/callback?state=someone-else&code=stolen",
        "http://127.0.0.1:4711/callback?code=no-state-at-all",
        "http://127.0.0.1:4711/callback?state=&code=empty-state",
        "not a url",
    ] {
        let pending = auth
            .begin_sign_in(ProviderId::Google, redirect_uri(4711))
            .unwrap();
        let error = auth.finish_sign_in(pending, bad).unwrap_err();
        assert!(
            matches!(error, SignInError::Failed { .. }),
            "{bad}: {error:?}"
        );
    }
    // A forged "the user said no" without our state is a failure, not a cancel.
    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    let error = auth
        .finish_sign_in(
            pending,
            "http://127.0.0.1:4711/callback?state=x&error=access_denied",
        )
        .unwrap_err();
    assert!(matches!(error, SignInError::Failed { .. }), "{error:?}");

    assert!(http.bodies().is_empty(), "a code was exchanged");
    assert_eq!(store.stored(ProviderId::Google), None);
    assert_eq!(auth.accounts()[0].state, AccountState::SignedOut);
}

#[test]
fn a_refused_consent_with_our_state_is_a_cancel() {
    let (auth, store, http) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Microsoft, redirect_uri(4711))
        .unwrap();
    let url = callback(&pending, "error=access_denied&error_description=no");
    let error = auth.finish_sign_in(pending, &url).unwrap_err();
    assert!(matches!(
        error,
        SignInError::Cancelled {
            provider: ProviderId::Microsoft
        }
    ));
    assert!(http.bodies().is_empty());
    assert_eq!(store.stored(ProviderId::Microsoft), None);
}

#[test]
fn signing_in_exchanges_the_code_with_the_verifier_and_stores_only_the_refresh_token() {
    let (auth, store, http) = setup();
    let account = signed_in_google(&auth, &http, 3600);
    assert_eq!(
        account,
        Account {
            provider: ProviderId::Google,
            label: Some("ada@example.com".into()),
            state: AccountState::SignedIn,
            remembered: true,
        }
    );
    assert_eq!(
        store.stored(ProviderId::Google).as_deref(),
        Some("refresh-1")
    );

    let form = http.form(0);
    assert_eq!(get(&form, "grant_type"), Some("authorization_code"));
    assert_eq!(get(&form, "code"), Some("the-code"));
    assert_eq!(
        get(&form, "redirect_uri"),
        Some("http://127.0.0.1:4711/callback")
    );
    assert_eq!(
        get(&form, "client_id"),
        Some("google-id.apps.googleusercontent.com")
    );
    // Google's desktop secret is non-secret but must be sent.
    assert_eq!(get(&form, "client_secret"), Some("google-not-secret"));
    assert!(get(&form, "code_verifier").is_some_and(|v| v.len() >= 43));

    // The access token is served from memory, with no second request.
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-1");
    assert_eq!(http.bodies().len(), 1);
}

#[test]
fn microsoft_sends_no_client_secret_and_reads_preferred_username() {
    let (auth, _, http) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Microsoft, redirect_uri(4711))
        .unwrap();
    http.reply(
        200,
        token_reply(
            Some("ms-refresh"),
            3600,
            serde_json::json!({ "preferred_username": "ada@contoso.com" }),
        ),
    );
    let url = callback(&pending, "code=c");
    let account = auth.finish_sign_in(pending, &url).unwrap();
    assert_eq!(account.label.as_deref(), Some("ada@contoso.com"));
    let form = http.form(0);
    assert_eq!(get(&form, "client_id"), Some("microsoft-id"));
    assert_eq!(get(&form, "client_secret"), None);
}

#[test]
fn a_reply_without_a_refresh_token_is_a_failure() {
    let (auth, store, http) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    http.reply(200, token_reply(None, 3600, serde_json::json!({})));
    let url = callback(&pending, "code=c");
    let error = auth.finish_sign_in(pending, &url).unwrap_err();
    assert!(matches!(error, SignInError::Failed { .. }), "{error:?}");
    assert_eq!(store.stored(ProviderId::Google), None);
}

#[test]
fn a_refused_or_unreachable_exchange_is_reported() {
    let (auth, _, http) = setup();
    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    http.reply(400, serde_json::json!({ "error": "invalid_grant" }));
    let url = callback(&pending, "code=c");
    let error = auth.finish_sign_in(pending, &url).unwrap_err();
    assert!(
        matches!(&error, SignInError::Failed { detail, .. } if detail.contains("invalid_grant"))
    );

    let pending = auth
        .begin_sign_in(ProviderId::Google, redirect_uri(4711))
        .unwrap();
    http.fail("connection refused");
    let url = callback(&pending, "code=c");
    let error = auth.finish_sign_in(pending, &url).unwrap_err();
    assert!(
        matches!(error, SignInError::Unreachable { .. }),
        "{error:?}"
    );
}

#[test]
fn without_a_keystore_the_sign_in_lasts_until_quit() {
    let store = Arc::new(MemoryStore::unavailable());
    let http = Arc::new(FakeHttp::default());
    let auth = auth_with(&store, &http);
    let account = signed_in_google(&auth, &http, 0);
    assert!(!account.remembered);
    assert_eq!(account.state, AccountState::SignedIn);

    // The in-memory refresh token still refreshes.
    http.reply(200, token_reply(None, 3600, serde_json::json!({})));
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-1");
    assert_eq!(get(&http.form(1), "refresh_token"), Some("refresh-1"));
    assert!(!auth.accounts()[0].remembered);
    // Signing out works even though the keystore cannot be reached.
    auth.sign_out(ProviderId::Google).unwrap();
    assert_eq!(auth.accounts()[0].state, AccountState::SignedOut);

    // A restart forgets it.
    let restarted = auth_with(&store, &http);
    assert_eq!(restarted.accounts()[0].state, AccountState::SignedOut);
}

#[test]
fn an_expiring_access_token_is_refreshed_silently_and_a_rotated_refresh_token_kept() {
    let (auth, store, http) = setup();
    signed_in_google(&auth, &http, 0);
    let mut reply = token_reply(Some("refresh-2"), 3600, serde_json::json!({}));
    reply["access_token"] = "access-2".into();
    http.reply(200, reply);
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-2");
    let form = http.form(1);
    assert_eq!(get(&form, "grant_type"), Some("refresh_token"));
    assert_eq!(get(&form, "refresh_token"), Some("refresh-1"));
    assert_eq!(get(&form, "client_secret"), Some("google-not-secret"));
    assert_eq!(
        store.stored(ProviderId::Google).as_deref(),
        Some("refresh-2")
    );
    // A refresh without an id_token keeps the label it had.
    assert_eq!(auth.accounts()[0].label.as_deref(), Some("ada@example.com"));
}

#[test]
fn a_rejected_refresh_token_is_sign_in_expired_until_signing_in_again() {
    let (auth, store, http) = setup();
    signed_in_google(&auth, &http, 0);
    http.reply(400, serde_json::json!({ "error": "invalid_grant" }));
    let error = auth.access_token(ProviderId::Google).unwrap_err();
    assert!(
        matches!(error, Error::SignInExpired { provider: "Google" }),
        "{error:?}"
    );
    assert_eq!(auth.accounts()[0].state, AccountState::Expired);
    // No second request: the verdict is kept.
    assert!(auth.access_token(ProviderId::Google).is_err());
    assert_eq!(http.bodies().len(), 2);
    // The dead token stays until sign-out or a new sign-in replaces it.
    assert!(store.stored(ProviderId::Google).is_some());

    signed_in_google(&auth, &http, 3600);
    assert_eq!(auth.accounts()[0].state, AccountState::SignedIn);
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-1");
}

#[test]
fn no_refresh_token_is_sign_in_expired_and_signed_out() {
    let (auth, _, http) = setup();
    let error = auth.access_token(ProviderId::Microsoft).unwrap_err();
    assert!(
        matches!(
            error,
            Error::SignInExpired {
                provider: "Microsoft"
            }
        ),
        "{error:?}"
    );
    let accounts = auth.accounts();
    assert_eq!(accounts.len(), 2);
    assert!(accounts.iter().all(|a| a.state == AccountState::SignedOut));
    assert!(http.bodies().is_empty());
}

#[test]
fn an_unreachable_token_endpoint_is_unreachable_not_expired() {
    let (auth, _, http) = setup();
    signed_in_google(&auth, &http, 0);
    http.fail("offline");
    let error = auth.access_token(ProviderId::Google).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Unreachable {
                provider: "Google",
                ..
            }
        ),
        "{error:?}"
    );
    http.reply(
        503,
        serde_json::json!({ "error": "temporarily_unavailable" }),
    );
    let error = auth.access_token(ProviderId::Google).unwrap_err();
    assert!(matches!(error, Error::Unreachable { .. }), "{error:?}");
    assert_eq!(auth.accounts()[0].state, AccountState::SignedIn);
}

#[test]
fn after_a_restart_the_stored_token_signs_in_silently() {
    let (auth, store, http) = setup();
    signed_in_google(&auth, &http, 3600);
    drop(auth);

    let restarted = auth_with(&store, &http);
    http.reply(
        200,
        token_reply(
            None,
            3600,
            serde_json::json!({ "email": "ada@example.com" }),
        ),
    );
    let google = &restarted.accounts()[0];
    assert_eq!(google.state, AccountState::SignedIn);
    assert_eq!(google.label.as_deref(), Some("ada@example.com"));
    assert_eq!(
        restarted.access_token(ProviderId::Google).unwrap(),
        "access-1"
    );
    assert_eq!(http.bodies().len(), 2, "one exchange, one refresh");

    // Offline after a restart: still signed in, account not known yet.
    let offline = auth_with(&store, &http);
    http.fail("offline");
    let google = &offline.accounts()[0];
    assert_eq!(google.state, AccountState::SignedIn);
    assert_eq!(google.label, None);
}

#[test]
fn signing_out_forgets_both_tokens() {
    let (auth, store, http) = setup();
    signed_in_google(&auth, &http, 3600);
    auth.sign_out(ProviderId::Google).unwrap();
    assert_eq!(store.stored(ProviderId::Google), None);
    assert!(matches!(
        auth.access_token(ProviderId::Google),
        Err(Error::SignInExpired { .. })
    ));
    assert_eq!(auth.accounts()[0].state, AccountState::SignedOut);
    // Signing out twice is fine.
    auth.sign_out(ProviderId::Google).unwrap();
}

#[test]
fn the_account_label_ignores_bad_tokens() {
    let token =
        id_token(serde_json::json!({ "email": " ada@example.com ", "preferred_username": "ada" }));
    assert_eq!(
        account_label(ProviderId::Google, Some(&token)).as_deref(),
        Some("ada@example.com")
    );
    assert_eq!(
        account_label(ProviderId::Microsoft, Some(&token)).as_deref(),
        Some("ada")
    );
    for bad in [
        None,
        Some(""),
        Some("one-part"),
        Some("a.!!!.c"),
        Some("a.bm90IGpzb24.c"),
    ] {
        assert_eq!(account_label(ProviderId::Google, bad), None, "{bad:?}");
    }
    let no_email = id_token(serde_json::json!({ "email": 5 }));
    assert_eq!(account_label(ProviderId::Google, Some(&no_email)), None);
}

#[test]
fn the_keystore_entry_is_named_after_the_bundle_id_and_provider() {
    assert_eq!(KEYRING_SERVICE, "pro.saleschat.meetai");
    assert_eq!(keyring_user(ProviderId::Google), "calendar-google");
    assert_eq!(keyring_user(ProviderId::Microsoft), "calendar-microsoft");
}

#[test]
fn the_client_secret_is_not_printed() {
    let client = clients(ProviderId::Google).unwrap();
    assert!(!format!("{client:?}").contains("google-not-secret"));
}

#[test]
fn a_rejected_access_token_is_refreshed_instead_of_handed_out_again() {
    let (auth, _, http) = setup();
    signed_in_google(&auth, &http, 3600);
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-1");
    // A token that is no longer the cached one changes nothing.
    auth.reject_access_token(ProviderId::Google, "some-older-token");
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-1");
    assert_eq!(http.bodies().len(), 1);

    let mut reply = token_reply(None, 3600, serde_json::json!({}));
    reply["access_token"] = "access-2".into();
    http.reply(200, reply);
    auth.reject_access_token(ProviderId::Google, "access-1");
    assert_eq!(auth.access_token(ProviderId::Google).unwrap(), "access-2");
    let form = http.form(1);
    assert_eq!(get(&form, "grant_type"), Some("refresh_token"));
}

#[test]
fn has_sign_in_reads_the_keystore_and_never_the_network() {
    let (auth, store, http) = setup();
    assert!(!auth.has_sign_in(ProviderId::Microsoft));
    store.save(ProviderId::Microsoft, "refresh-1").unwrap();
    assert!(auth.has_sign_in(ProviderId::Microsoft));
    assert!(!auth.has_sign_in(ProviderId::Google));
    assert!(http.bodies().is_empty());

    // An expired sign-in still counts: the provider reports it as expired.
    http.reply(400, serde_json::json!({ "error": "invalid_grant" }));
    assert!(auth.access_token(ProviderId::Microsoft).is_err());
    assert!(auth.has_sign_in(ProviderId::Microsoft));

    // Kept in memory only (no keystore) counts too.
    let store = Arc::new(MemoryStore::unavailable());
    let http = Arc::new(FakeHttp::default());
    let auth = auth_with(&store, &http);
    signed_in_google(&auth, &http, 3600);
    assert!(auth.has_sign_in(ProviderId::Google));
}
