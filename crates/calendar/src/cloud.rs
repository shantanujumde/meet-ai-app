//! What the cloud providers ([`crate::google`], [`crate::microsoft`]) share
//! (TUR-90): where the access token comes from, and the paged, authorised
//! GET loop every read runs.
//!
//! - [`TokenSource`]: a token that should work now, and a fresh one when the
//!   provider answered 401. The app answers it from
//!   [`CalendarAuth`], bound to one provider by [`IntoTokenSource`].
//! - [`read_pages`]: GET a page with the bearer token, renew the token once
//!   on a 401, follow the next-page link up to [`MAX_PAGES`] times.

use std::sync::Arc;

use oauth2::http::{Method, Request, StatusCode, header};
use oauth2::url::Url;
use oauth2::{HttpRequest, HttpResponse};

use crate::Error;
use crate::oauth::{CalendarAuth, HttpClient, ProviderId};

/// A cap on pages per read, so a server that keeps sending a next page
/// cannot keep a read going forever.
pub(crate) const MAX_PAGES: usize = 40;

/// Where a cloud provider gets its access token.
pub trait TokenSource: Send + Sync {
    /// A token that should work now. [`Error::SignInExpired`] when there is
    /// no sign-in or it was rejected.
    fn access_token(&self) -> Result<String, Error>;

    /// The provider answered 401 with `rejected`: a fresh token, refreshed
    /// rather than cached.
    fn renew_access_token(&self, rejected: &str) -> Result<String, Error>;
}

impl<T: TokenSource + ?Sized> TokenSource for Arc<T> {
    fn access_token(&self) -> Result<String, Error> {
        (**self).access_token()
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        (**self).renew_access_token(rejected)
    }
}

/// What a provider's `new` takes its tokens from: any [`TokenSource`] as
/// is, or a [`CalendarAuth`] (owned or shared), which serves every provider
/// and so is bound to the one asking.
pub trait IntoTokenSource {
    fn into_token_source(self: Box<Self>, provider: ProviderId) -> Box<dyn TokenSource>;
}

impl<T: TokenSource + 'static> IntoTokenSource for T {
    fn into_token_source(self: Box<Self>, _provider: ProviderId) -> Box<dyn TokenSource> {
        self
    }
}

impl IntoTokenSource for dyn TokenSource {
    fn into_token_source(self: Box<Self>, _provider: ProviderId) -> Box<dyn TokenSource> {
        self
    }
}

impl IntoTokenSource for CalendarAuth {
    fn into_token_source(self: Box<Self>, provider: ProviderId) -> Box<dyn TokenSource> {
        Box::new(Bound {
            auth: Arc::from(self),
            provider,
        })
    }
}

impl IntoTokenSource for Arc<CalendarAuth> {
    fn into_token_source(self: Box<Self>, provider: ProviderId) -> Box<dyn TokenSource> {
        Box::new(Bound {
            auth: *self,
            provider,
        })
    }
}

/// A [`CalendarAuth`] answering for one provider.
struct Bound {
    auth: Arc<CalendarAuth>,
    provider: ProviderId,
}

impl TokenSource for Bound {
    fn access_token(&self) -> Result<String, Error> {
        self.auth.access_token(self.provider)
    }

    fn renew_access_token(&self, rejected: &str) -> Result<String, Error> {
        self.auth.renew_access_token(self.provider, rejected)
    }
}

/// How one cloud API is called and named in errors.
pub(crate) struct Api {
    /// The provider in errors: "Google", "Microsoft".
    pub provider: &'static str,
    /// The API in "could not build the … request": "Google Calendar".
    pub request_name: &'static str,
    /// Who "sent more than N pages": "Google", "Graph".
    pub pages_name: &'static str,
    /// Extra request headers, after `Authorization` and `Accept`.
    pub headers: &'static [(&'static str, &'static str)],
}

/// Read every page from `first`. `page` turns one response into its items
/// and the next page's URL (`None` on the last page).
pub(crate) fn read_pages<T>(
    api: &Api,
    tokens: &dyn TokenSource,
    http: &dyn HttpClient,
    first: Url,
    mut page: impl FnMut(&HttpResponse) -> Result<(Vec<T>, Option<Url>), Error>,
) -> Result<Vec<T>, Error> {
    let mut url = first;
    let mut token = tokens.access_token()?;
    let mut renewed = false;
    let mut found = Vec::new();
    for _ in 0..MAX_PAGES {
        let mut response = get(api, http, &url, &token)?;
        if response.status() == StatusCode::UNAUTHORIZED && !renewed {
            // Revoked, or expired early: refresh once and try again.
            renewed = true;
            token = tokens.renew_access_token(&token)?;
            response = get(api, http, &url, &token)?;
        }
        let (items, next) = page(&response)?;
        found.extend(items);
        match next {
            Some(next) => url = next,
            None => return Ok(found),
        }
    }
    Err(unreachable(
        api.provider,
        format!(
            "{} sent more than {MAX_PAGES} pages of events",
            api.pages_name
        ),
    ))
}

/// One GET with the bearer token. Transport failures are
/// [`Error::Unreachable`]; every HTTP status comes back as is.
fn get(api: &Api, http: &dyn HttpClient, url: &Url, token: &str) -> Result<HttpResponse, Error> {
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(url.as_str())
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::ACCEPT, "application/json");
    for (name, value) in api.headers {
        builder = builder.header(*name, *value);
    }
    let request: HttpRequest = builder
        .body(Vec::new())
        // The detail never includes the request: it holds the token.
        .map_err(|_| {
            unreachable(
                api.provider,
                format!("could not build the {} request", api.request_name),
            )
        })?;
    http.execute(request)
        .map_err(|error| unreachable(api.provider, error.to_string()))
}

/// [`Error::Unreachable`] for `provider`.
pub(crate) fn unreachable(provider: &'static str, detail: impl Into<String>) -> Error {
    Error::Unreachable {
        provider,
        detail: detail.into(),
    }
}
