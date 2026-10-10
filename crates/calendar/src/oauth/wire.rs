//! The `oauth2` crate's types and the small functions that build a client,
//! read a token response and describe a failed request. Split out of
//! `mod.rs` to keep that file under the size limit; nothing here holds state.

use std::time::{Duration, Instant};

use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
};
use oauth2::{
    AuthType, AuthUrl, Client, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl,
    RequestTokenError, StandardRevocableToken, TokenResponse as _, TokenUrl,
};

use super::{HttpError, OAuthClient, ProviderId, SignInError, TokenResponse};

/// Assumed lifetime when the provider does not say (both do today: ~1 hour).
const DEFAULT_LIFETIME: Duration = Duration::from_secs(60 * 60);

pub(super) type OAuth2Client = Client<
    BasicErrorResponse,
    TokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

/// The access token and when it runs out.
pub(super) fn access_of(response: &TokenResponse) -> (String, Instant) {
    let lifetime = response.expires_in().unwrap_or(DEFAULT_LIFETIME);
    (
        response.access_token().secret().clone(),
        Instant::now() + lifetime,
    )
}

pub(super) fn build_client(
    provider: ProviderId,
    client: &OAuthClient,
    redirect_uri: Option<&str>,
) -> Result<OAuth2Client, SignInError> {
    let failed = |detail: String| SignInError::Failed { provider, detail };
    let endpoints = provider.endpoints();
    let auth_url =
        AuthUrl::new(endpoints.auth_url.to_owned()).map_err(|error| failed(error.to_string()))?;
    let token_url =
        TokenUrl::new(endpoints.token_url.to_owned()).map_err(|error| failed(error.to_string()))?;
    let mut oauth: OAuth2Client = Client::new(ClientId::new(client.client_id.trim().to_owned()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        // Google documents the client id and secret in the body for
        // installed apps; with no secret (Microsoft) it is the same anyway.
        .set_auth_type(AuthType::RequestBody);
    if let Some(secret) = client
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        oauth = oauth.set_client_secret(ClientSecret::new(secret.to_owned()));
    }
    if let Some(redirect_uri) = redirect_uri {
        let redirect =
            RedirectUrl::new(redirect_uri.to_owned()).map_err(|error| failed(error.to_string()))?;
        oauth = oauth.set_redirect_uri(redirect);
    }
    Ok(oauth)
}

/// A failed code exchange, as a [`SignInError`].
pub(super) fn exchange_error(
    provider: ProviderId,
    error: RequestTokenError<HttpError, BasicErrorResponse>,
) -> SignInError {
    match error {
        RequestTokenError::Request(error) => SignInError::Unreachable {
            provider,
            detail: error.to_string(),
        },
        error => SignInError::Failed {
            provider,
            detail: request_error_detail(&error),
        },
    }
}

/// A token request error in one line, without the response body (it can
/// echo tokens back).
pub(super) fn request_error_detail(
    error: &RequestTokenError<HttpError, BasicErrorResponse>,
) -> String {
    match error {
        RequestTokenError::ServerResponse(response) => match response.error_description() {
            Some(description) => format!("{}: {description}", response.error()),
            None => response.error().to_string(),
        },
        RequestTokenError::Request(error) => error.to_string(),
        RequestTokenError::Parse(error, _) => format!("unreadable reply: {error}"),
        RequestTokenError::Other(detail) => detail.clone(),
    }
}
