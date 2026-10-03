//! The `calendar` section of `config.jsonc` (SPEC §3.5, providers per L13 as
//! amended by A12): which calendars the app reads upcoming meetings from, how
//! often, and the OAuth clients for Google and Microsoft sign-in (SPEC §8.1).
//!
//! An unknown provider name is an error, never silently dropped: the same rule
//! A4 set for `transcription.engine`. If you wrote `"googel"`, you want to be
//! told, not to find the app quietly reading only EventKit. Like
//! `transcription`, though, the app-facing reader [`calendar`] logs the error
//! and runs with the defaults, so a typo never stops the app from starting.
//! [`parse_calendar`] returns the error for a caller (say a settings screen)
//! that wants to show it.
//!
//! The app's `CalendarState` (TUR-28) calls [`calendar`] and
//! [`CalendarConfig::available_providers`] on every read.
//!
//! A12 (2026-10-03) dropped ICS: `"ics"` is now an unknown provider name, and
//! `ics_urls` is still parsed, so an old config loads, but nothing reads it.
//! Sign-in (TUR-44) reads [`CalendarConfig::google`] and
//! [`CalendarConfig::microsoft`] on every sign-in and refresh, so pasting a
//! client id needs no restart.

use std::fmt;

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::read_section;

/// One calendar source (SPEC L13, §2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// The macOS Calendar app's store, through EventKit. Reads every account
    /// already set up in Calendar.app.
    EventKit,
    /// Google Calendar over OAuth.
    Google,
    /// Microsoft Graph over OAuth.
    Microsoft,
}

impl Provider {
    /// Every value, in the order `config.schema.json` lists them.
    pub const ALL: [Self; 3] = [Self::EventKit, Self::Google, Self::Microsoft];

    /// The spelling in `config.jsonc`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EventKit => "eventkit",
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }

    /// Whether this build can read the provider. EventKit and Microsoft
    /// (TUR-47) are built; Google is a valid name that is skipped with a log
    /// line until TUR-48.
    pub fn is_available(self) -> bool {
        matches!(self, Self::EventKit | Self::Microsoft)
    }

    fn from_config(name: &str) -> Result<Self, ConfigError> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == name)
            .ok_or_else(|| {
                ConfigError::Invalid(format!(
                    "calendar.providers {name:?} is not one of {}",
                    Self::ALL.map(Self::as_str).join(", ")
                ))
            })
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `calendar` in `config.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarConfig {
    /// The sources to read, in the order written, without repeats.
    pub providers: Vec<Provider>,
    /// `.ics` feed links. Ignored since A12 dropped ICS; kept so old configs
    /// still load.
    pub ics_urls: Vec<String>,
    /// How often to re-read the calendars, in minutes. Never 0.
    pub refresh_minutes: u32,
    /// `calendar.google`: the OAuth client for Google sign-in.
    pub google: OAuthClientConfig,
    /// `calendar.microsoft`: the OAuth client for Microsoft sign-in. Never
    /// has a `client_secret` (a public client).
    pub microsoft: OAuthClientConfig,
}

/// `calendar.google` / `calendar.microsoft`: an OAuth client registered by
/// whoever builds or runs the app (SETUP.md, "Calendar sign-in"). `None` (or
/// an empty string) means not set up, and that provider's sign-in says so.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OAuthClientConfig {
    pub client_id: Option<String>,
    /// Google desktop clients only: a client secret that Google itself calls
    /// non-secret, which must still be sent with each token request.
    pub client_secret: Option<String>,
}

impl fmt::Debug for OAuthClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClientConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "…"))
            .finish()
    }
}

/// `calendar.google` as written.
#[derive(Debug, Default, Deserialize)]
struct RawGoogle {
    client_id: Option<String>,
    client_secret: Option<String>,
}

/// `calendar.microsoft` as written. No secret: a public client.
#[derive(Debug, Default, Deserialize)]
struct RawMicrosoft {
    client_id: Option<String>,
}

/// A set, non-blank value, trimmed.
fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            providers: vec![Provider::EventKit],
            ics_urls: Vec::new(),
            refresh_minutes: 15,
            google: OAuthClientConfig::default(),
            microsoft: OAuthClientConfig::default(),
        }
    }
}

impl CalendarConfig {
    /// The providers this build can read. A known provider that is not built
    /// yet (google, microsoft) is logged as "not available yet" and
    /// skipped, so the rest still work.
    pub fn available_providers(&self) -> Vec<Provider> {
        self.providers
            .iter()
            .copied()
            .filter(|provider| {
                let available = provider.is_available();
                if !available {
                    tracing::warn!(%provider, "calendar provider is not available yet; skipping it");
                }
                available
            })
            .collect()
    }
}

/// `calendar` as written. Every key optional, so a missing one is the default.
#[derive(Debug, Default, Deserialize)]
struct RawCalendar {
    // Strings, not `Provider`, so an unknown name gets a message naming the
    // allowed list rather than a generic serde one.
    providers: Option<Vec<String>>,
    ics_urls: Option<Vec<String>>,
    refresh_minutes: Option<u32>,
    google: Option<RawGoogle>,
    microsoft: Option<RawMicrosoft>,
}

/// `calendar` from the text of `config.jsonc`. Empty text, or no `calendar`
/// key, is all defaults. An unknown provider name or a `refresh_minutes` of 0
/// is an error.
pub fn parse_calendar(raw: &str) -> Result<CalendarConfig, ConfigError> {
    let calendar: RawCalendar = read_section(raw, "calendar")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    let defaults = CalendarConfig::default();
    let providers = match calendar.providers {
        None => defaults.providers,
        Some(names) => {
            let mut providers = Vec::with_capacity(names.len());
            for name in &names {
                let provider = Provider::from_config(name)?;
                if !providers.contains(&provider) {
                    providers.push(provider);
                }
            }
            providers
        }
    };
    let refresh_minutes = calendar.refresh_minutes.unwrap_or(defaults.refresh_minutes);
    if refresh_minutes == 0 {
        return Err(ConfigError::Invalid(
            "calendar.refresh_minutes must be at least 1".into(),
        ));
    }
    Ok(CalendarConfig {
        providers,
        ics_urls: calendar.ics_urls.unwrap_or(defaults.ics_urls),
        refresh_minutes,
        google: calendar
            .google
            .map(|google| OAuthClientConfig {
                client_id: non_blank(google.client_id),
                client_secret: non_blank(google.client_secret),
            })
            .unwrap_or_default(),
        microsoft: calendar
            .microsoft
            .map(|microsoft| OAuthClientConfig {
                client_id: non_blank(microsoft.client_id),
                client_secret: None,
            })
            .unwrap_or_default(),
    })
}

/// [`parse_calendar`], with a bad section logged and replaced by the
/// defaults, the way `transcription` behaves: startup never fails on it.
fn calendar_or_defaults(raw: &str) -> CalendarConfig {
    parse_calendar(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's calendar section is not valid; using defaults");
        CalendarConfig::default()
    })
}

/// `calendar` from `~/Meetings/.app/config.jsonc`, or the SPEC §3.5 defaults
/// if the file or section is missing or not valid (logged).
#[allow(dead_code)] // TUR-27/TUR-28 (calendar refresh loop) call this.
pub fn calendar() -> CalendarConfig {
    calendar_or_defaults(&super::raw_or_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_section_is_the_spec_3_5_defaults() {
        let defaults = CalendarConfig {
            providers: vec![Provider::EventKit],
            ics_urls: vec![],
            refresh_minutes: 15,
            google: OAuthClientConfig::default(),
            microsoft: OAuthClientConfig::default(),
        };
        assert_eq!(CalendarConfig::default(), defaults);
        for raw in [
            "",
            "// only a comment\n",
            "{}",
            r#"{ "calendar": {} }"#,
            r#"{ "detection": { "calendar": false } }"#,
        ] {
            assert_eq!(parse_calendar(raw).unwrap(), defaults, "{raw:?}");
        }
    }

    #[test]
    fn every_key_overrides() {
        let calendar = parse_calendar(
            r#"{
                // JSONC: comments allowed
                "calendar": {
                    "providers": ["microsoft", "eventkit", "google"],
                    "ics_urls": ["https://example.com/a.ics"],
                    "refresh_minutes": 5,
                    "google": {
                        "client_id": " 123-abc.apps.googleusercontent.com ",
                        "client_secret": "GOCSPX-not-secret"
                    },
                    "microsoft": { "client_id": "00000000-0000-0000-0000-000000000000" }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            calendar,
            CalendarConfig {
                providers: vec![Provider::Microsoft, Provider::EventKit, Provider::Google],
                ics_urls: vec!["https://example.com/a.ics".into()],
                refresh_minutes: 5,
                google: OAuthClientConfig {
                    client_id: Some("123-abc.apps.googleusercontent.com".into()),
                    client_secret: Some("GOCSPX-not-secret".into()),
                },
                microsoft: OAuthClientConfig {
                    client_id: Some("00000000-0000-0000-0000-000000000000".into()),
                    client_secret: None,
                },
            }
        );
    }

    #[test]
    fn a_missing_key_keeps_its_default_beside_set_ones() {
        let calendar = parse_calendar(r#"{ "calendar": { "refresh_minutes": 60 } }"#).unwrap();
        assert_eq!(calendar.providers, vec![Provider::EventKit]);
        assert_eq!(calendar.refresh_minutes, 60);
    }

    #[test]
    fn an_empty_provider_list_means_no_calendars() {
        let calendar = parse_calendar(r#"{ "calendar": { "providers": [] } }"#).unwrap();
        assert!(calendar.providers.is_empty());
    }

    #[test]
    fn a_repeated_provider_is_read_once() {
        let calendar =
            parse_calendar(r#"{ "calendar": { "providers": ["eventkit", "eventkit"] } }"#).unwrap();
        assert_eq!(calendar.providers, vec![Provider::EventKit]);
    }

    #[test]
    fn an_unknown_provider_is_an_error_naming_it_and_the_allowed_list() {
        let error = parse_calendar(r#"{ "calendar": { "providers": ["eventkit", "googel"] } }"#)
            .unwrap_err();
        assert!(matches!(&error, ConfigError::Invalid(_)), "{error:?}");
        let message = error.to_string();
        assert!(
            message.contains("calendar.providers \"googel\""),
            "{message}"
        );
        assert!(
            message.ends_with("is not one of eventkit, google, microsoft"),
            "{message}"
        );
    }

    #[test]
    fn ics_was_dropped_so_it_is_an_unknown_provider_but_old_ics_urls_still_load() {
        let error =
            parse_calendar(r#"{ "calendar": { "providers": ["eventkit", "ics"] } }"#).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("calendar.providers \"ics\""), "{message}");
        assert!(
            message.ends_with("eventkit, google, microsoft"),
            "{message}"
        );

        let calendar =
            parse_calendar(r#"{ "calendar": { "ics_urls": ["https://example.com/a.ics"] } }"#)
                .unwrap();
        assert_eq!(calendar.providers, vec![Provider::EventKit]);
        assert_eq!(
            calendar.ics_urls,
            vec!["https://example.com/a.ics".to_owned()]
        );
    }

    #[test]
    fn a_blank_client_id_is_not_set_and_microsoft_takes_no_secret() {
        let calendar = parse_calendar(
            r#"{ "calendar": {
                "google": { "client_id": "   ", "client_secret": "" },
                "microsoft": { "client_id": "ms-id", "client_secret": "ignored" }
            } }"#,
        )
        .unwrap();
        assert_eq!(calendar.google, OAuthClientConfig::default());
        assert_eq!(
            calendar.microsoft,
            OAuthClientConfig {
                client_id: Some("ms-id".into()),
                client_secret: None,
            }
        );
    }

    #[test]
    fn a_bad_client_id_is_logged_and_the_defaults_used() {
        for raw in [
            r#"{ "calendar": { "google": { "client_id": 5 } } }"#,
            r#"{ "calendar": { "microsoft": "ms-id" } }"#,
        ] {
            assert!(parse_calendar(raw).is_err(), "{raw:?}");
            assert_eq!(
                calendar_or_defaults(raw),
                CalendarConfig::default(),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn the_client_secret_is_not_printed() {
        let client = OAuthClientConfig {
            client_id: Some("id".into()),
            client_secret: Some("GOCSPX-not-secret".into()),
        };
        assert!(!format!("{client:?}").contains("GOCSPX"));
    }

    #[test]
    fn the_app_reader_logs_a_bad_section_and_runs_with_defaults() {
        for raw in [
            r#"{ "calendar": { "providers": ["outlook"] } }"#,
            r#"{ "calendar": { "refresh_minutes": 0 } }"#,
            r#"{ "calendar": { "refresh_minutes": -5 } }"#,
            r#"{ "calendar": { "providers": "eventkit" } }"#,
            r#"{ "calendar": null }"#,
            "{ not json",
        ] {
            assert!(parse_calendar(raw).is_err(), "{raw:?}");
            assert_eq!(
                calendar_or_defaults(raw),
                CalendarConfig::default(),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_known_but_unbuilt_provider_is_skipped_not_rejected() {
        let calendar = parse_calendar(
            r#"{ "calendar": { "providers": ["google", "eventkit", "microsoft"] } }"#,
        )
        .unwrap();
        assert_eq!(
            calendar.available_providers(),
            vec![Provider::EventKit, Provider::Microsoft]
        );
        let calendar = parse_calendar(r#"{ "calendar": { "providers": ["google"] } }"#).unwrap();
        assert!(calendar.available_providers().is_empty());
    }

    #[test]
    fn every_provider_name_round_trips() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_config(provider.as_str()).unwrap(), provider);
        }
    }

    #[test]
    fn the_provider_list_matches_the_schema() {
        let schema: serde_json::Value = serde_json::from_str(super::super::file::SCHEMA).unwrap();
        let listed = &schema["properties"]["calendar"]["properties"]["providers"]["items"]["enum"];
        let names: Vec<_> = Provider::ALL.map(Provider::as_str).into();
        assert_eq!(listed, &serde_json::json!(names));
    }
}
