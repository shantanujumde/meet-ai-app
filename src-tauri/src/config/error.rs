//! Why a section of `config.jsonc` could not be read or written: the one
//! error every section reader and writer returns (TUR-176; it used to live
//! in `agent_section.rs`).

use std::fmt;

use super::agent_section::Harness;
use crate::error::UiError;

/// Why the `agent` or `tickets` section could not be read or written.
#[derive(Debug)]
pub enum ConfigError {
    /// `agent.harness` names a CLI the app does not know.
    UnknownHarness(String),
    /// The file is not valid JSONC, or a key has the wrong type or range.
    Invalid(String),
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The meetings folder itself could not be found. Kept as the original
    /// `UiError` so its own kind (say `no-home-dir`) reaches the UI.
    Root(UiError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHarness(name) => {
                let known: Vec<_> = Harness::ALL.iter().map(|h| h.as_str()).collect();
                write!(
                    f,
                    "config.jsonc: agent.harness {name:?} is not one of {}",
                    known.join(", ")
                )
            }
            Self::Invalid(detail) => write!(f, "config.jsonc: {detail}"),
            Self::Io(error) => write!(f, "config.jsonc: {error}"),
            Self::Root(error) => write!(f, "config.jsonc: {}", error.message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
