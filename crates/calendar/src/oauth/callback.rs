//! Reading what comes back: the loopback callback URL, and the account label
//! in the id_token.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oauth2::url::Url;

use super::{ProviderId, SignInError};

/// Check the URL the browser was redirected to, and return the
/// authorization code.
///
/// `state` is checked first, before anything else in the URL is trusted: any
/// local process can send a request to the loopback port, so a reply that
/// does not carry our `state` is [`SignInError::Failed`] whatever else it
/// says. Only then is an `error=access_denied` a [`SignInError::Cancelled`].
pub fn check_callback(
    provider: ProviderId,
    callback_url: &str,
    expected_state: &str,
) -> Result<String, SignInError> {
    let failed = |detail: &str| SignInError::Failed {
        provider,
        detail: detail.to_owned(),
    };
    let url = Url::parse(callback_url).map_err(|_| failed("the reply was not a URL"))?;
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    let state_matches = param("state")
        .is_some_and(|state| constant_time_eq(state.as_bytes(), expected_state.as_bytes()));
    if !state_matches {
        return Err(failed(
            "the reply did not carry this sign-in's state, so it was ignored and nothing was saved",
        ));
    }
    if let Some(error) = param("error") {
        if error == "access_denied" {
            return Err(SignInError::Cancelled { provider });
        }
        let detail = match param("error_description") {
            Some(description) => format!("{error}: {description}"),
            None => error,
        };
        return Err(SignInError::Failed { provider, detail });
    }
    param("code")
        .filter(|code| !code.is_empty())
        .ok_or_else(|| failed("the reply had no authorization code"))
}

/// Equal bytes, compared without stopping at the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The account to show: the `email` claim (Google) or `preferred_username`,
/// else `email` (Microsoft), from the id_token's payload.
///
/// The signature is **not** checked: this is a label on the settings screen
/// and is never used to decide anything. The token came straight from the
/// provider's token endpoint over TLS.
pub fn account_label(provider: ProviderId, id_token: Option<&str>) -> Option<String> {
    let payload = id_token?.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    provider
        .endpoints()
        .account_claims
        .iter()
        .find_map(|claim| {
            claims
                .get(claim)?
                .as_str()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
        })
}
