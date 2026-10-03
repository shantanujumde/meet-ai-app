//! Where meetings come from: the Settings card's commands (TUR-49, SPEC A12).
//!
//! On macOS the sources are the Calendar app (EventKit, on by default) and
//! the optional Google and Microsoft sign-ins; on Windows and Linux only the
//! two sign-ins. [`calendar_sources`] tells the card which of these this OS
//! offers and which are set up; the account itself (signed in, expired,
//! signed out) comes from [`signin::calendar_accounts`].
//!
//! Connecting a sign-in also adds it to `calendar.providers`, and
//! disconnecting removes it, through the comment-keeping config writer, so the
//! Today pane, reminders, auto-titles and the brief read it with no restart.

use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use super::signin::{self, CalendarAccount, SignInProvider};
use crate::config::{self, CalendarConfig, Provider};
use crate::error::UiError;
use crate::folder_move::FolderGate;

/// What the Settings card shows before any account is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSources {
    /// This OS has a Calendar app to read (macOS only).
    pub calendar_app_available: bool,
    /// `"eventkit"` is in `calendar.providers`.
    pub calendar_app: bool,
    /// The sign-ins with a client id in `config.jsonc`. One without it
    /// cannot sign in, and the card says which key to add instead.
    pub configured: Vec<SignInProvider>,
    /// The sign-ins named in `calendar.providers`, so read when signed in.
    pub connected: Vec<SignInProvider>,
}

impl CalendarSources {
    pub fn new(config: &CalendarConfig, calendar_app_available: bool) -> Self {
        let configured = [
            (SignInProvider::Google, &config.google),
            (SignInProvider::Microsoft, &config.microsoft),
        ]
        .into_iter()
        .filter(|(_, client)| client.client_id.is_some())
        .map(|(provider, _)| provider)
        .collect();
        let connected = config
            .providers
            .iter()
            .filter_map(|provider| match provider {
                Provider::EventKit => None,
                Provider::Google => Some(SignInProvider::Google),
                Provider::Microsoft => Some(SignInProvider::Microsoft),
            })
            .collect();
        Self {
            calendar_app_available,
            calendar_app: calendar_app_available && config.providers.contains(&Provider::EventKit),
            configured,
            connected,
        }
    }
}

impl From<SignInProvider> for Provider {
    fn from(provider: SignInProvider) -> Self {
        match provider {
            SignInProvider::Google => Self::Google,
            SignInProvider::Microsoft => Self::Microsoft,
        }
    }
}

/// The message for turning the Calendar app on where there is none.
pub const NO_CALENDAR_APP: &str =
    "This system has no Calendar app. Sign in with Google or Microsoft instead.";

fn current() -> CalendarSources {
    CalendarSources::new(&config::calendar(), crate::platform::HAS_CALENDAR_APP)
}

/// Change `calendar.providers` on the blocking pool, through the
/// [`FolderGate`] (the config lives under the meetings root).
async fn edit_providers(
    app: AppHandle,
    edit: impl FnOnce(&mut Vec<Provider>) + Send + 'static,
) -> Result<CalendarSources, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<FolderGate>().writing(|| {
            let saved = config::set_calendar_providers(edit)?;
            Ok(CalendarSources::new(
                &saved,
                crate::platform::HAS_CALENDAR_APP,
            ))
        })
    })
    .await
    .map_err(|error| UiError::app("task-failed", error.to_string()))?
}

/// Which calendar sources this OS offers and which are set up. A config
/// read only; never the network or the keystore.
#[tauri::command]
#[specta::specta]
pub async fn calendar_sources() -> CalendarSources {
    tauri::async_runtime::spawn_blocking(current)
        .await
        .unwrap_or_else(|_| CalendarSources::new(&CalendarConfig::default(), false))
}

/// Turn the Calendar app on or off (macOS). On first, so its ids win when a
/// meeting is also in a signed-in calendar. Off macOS, turning it on is the
/// error kind `calendar-app-unavailable`.
#[tauri::command]
#[specta::specta]
pub async fn set_calendar_app(app: AppHandle, on: bool) -> Result<CalendarSources, UiError> {
    if on && !crate::platform::HAS_CALENDAR_APP {
        return Err(UiError::app("calendar-app-unavailable", NO_CALENDAR_APP));
    }
    edit_providers(app, move |providers| {
        providers.retain(|provider| *provider != Provider::EventKit);
        if on {
            providers.insert(0, Provider::EventKit);
        }
    })
    .await
}

/// Sign in to Google or Microsoft in the browser, then read that calendar:
/// [`signin::calendar_sign_in`] and the provider added to
/// `calendar.providers`. Its errors, plus the config writer's.
#[tauri::command]
#[specta::specta]
pub async fn calendar_connect(
    app: AppHandle,
    provider: SignInProvider,
) -> Result<CalendarAccount, UiError> {
    let account = signin::calendar_sign_in(app.clone(), provider).await?;
    edit_providers(app, move |providers| providers.push(provider.into())).await?;
    Ok(account)
}

/// Sign out and stop reading that calendar: [`signin::calendar_sign_out`],
/// then the provider removed from `calendar.providers`. The sign-out runs
/// first, so a config that cannot be written never leaves a token behind.
#[tauri::command]
#[specta::specta]
pub async fn calendar_disconnect(
    app: AppHandle,
    provider: SignInProvider,
) -> Result<CalendarSources, UiError> {
    signin::calendar_sign_out(app.clone(), provider).await?;
    let provider = Provider::from(provider);
    edit_providers(app, move |providers| {
        providers.retain(|each| *each != provider)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_calendar;

    fn config(raw: &str) -> CalendarConfig {
        parse_calendar(raw).unwrap()
    }

    #[test]
    fn a_mac_shows_the_calendar_app_and_which_sign_ins_are_set_up() {
        let mac = CalendarSources::new(
            &CalendarConfig {
                providers: vec![Provider::EventKit, Provider::Microsoft],
                ..config(r#"{ "calendar": { "providers": [], "google": { "client_id": "g" } } }"#)
            },
            true,
        );
        assert_eq!(
            mac,
            CalendarSources {
                calendar_app_available: true,
                calendar_app: true,
                configured: vec![SignInProvider::Google],
                connected: vec![SignInProvider::Microsoft],
            }
        );
    }

    #[test]
    fn off_macos_there_is_no_calendar_app_whatever_the_list_says() {
        let other = CalendarSources::new(&CalendarConfig::defaults_for(true), false);
        assert!(!other.calendar_app_available);
        assert!(!other.calendar_app);
        assert!(other.configured.is_empty());
        assert!(other.connected.is_empty());
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let sources = CalendarSources::new(&CalendarConfig::defaults_for(true), true);
        assert_eq!(
            serde_json::to_value(&sources).unwrap(),
            serde_json::json!({
                "calendarAppAvailable": true,
                "calendarApp": true,
                "configured": [],
                "connected": [],
            })
        );
    }
}
