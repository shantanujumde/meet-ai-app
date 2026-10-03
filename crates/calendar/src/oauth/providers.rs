//! The two sign-in providers and their fixed endpoints (SPEC §2.7, A12).
//!
//! Everything here is public, documented data from each provider's
//! native-app guide. The client id (and Google's non-secret client secret) are
//! not here: they come from `config.jsonc` (SPEC §8.1), so a verified client
//! can be swapped in without a rebuild.

use std::fmt;

/// A cloud calendar that needs a sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderId {
    Google,
    Microsoft,
}

impl ProviderId {
    /// Both providers, in the order the settings screen lists them.
    pub const ALL: [Self; 2] = [Self::Google, Self::Microsoft];

    /// The spelling in code, config (`calendar.google`) and the keystore.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }

    /// The name shown to people: "Google", "Microsoft".
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
        }
    }

    /// The provider's fixed endpoints and scopes.
    pub fn endpoints(self) -> &'static Endpoints {
        match self {
            Self::Google => &GOOGLE,
            Self::Microsoft => &MICROSOFT,
        }
    }

    /// The `config.jsonc` key that holds this provider's client id, for the
    /// "not configured" message.
    pub fn client_id_key(self) -> &'static str {
        match self {
            Self::Google => "calendar.google.client_id",
            Self::Microsoft => "calendar.microsoft.client_id",
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One provider's authorization server.
#[derive(Debug)]
pub struct Endpoints {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    /// Read-only calendar access, plus what names the account.
    pub scopes: &'static [&'static str],
    /// Extra query parameters on the authorization URL.
    pub extra_auth_params: &'static [(&'static str, &'static str)],
    /// The id_token claim shown as the account label.
    pub account_claim: &'static str,
}

/// Google's installed-app flow. `access_type=offline` is what returns a
/// refresh token at all; `prompt=consent` makes Google return one again on a
/// second sign-in, instead of only the first time ever.
static GOOGLE: Endpoints = Endpoints {
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    scopes: &[
        "openid",
        "email",
        "https://www.googleapis.com/auth/calendar.events.readonly",
    ],
    extra_auth_params: &[("access_type", "offline"), ("prompt", "consent")],
    account_claim: "email",
};

/// Microsoft identity platform, `common` tenant: personal and work accounts.
/// `offline_access` is what returns a refresh token.
static MICROSOFT: Endpoints = Endpoints {
    auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    scopes: &[
        "openid",
        "email",
        "offline_access",
        "https://graph.microsoft.com/Calendars.Read",
    ],
    extra_auth_params: &[],
    account_claim: "preferred_username",
};
