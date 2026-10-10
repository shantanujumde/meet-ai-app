//! What every section of `config.jsonc` used to repeat (TUR-176): read it,
//! fall back to its defaults, read it from the file, write it back.
//!
//! - [`Section`]: a Settings section read key by key (TUR-155, `keyed.rs`).
//!   A section says its name, how it decodes, and which keys to write; the
//!   reader, the checked reader and the comment-keeping writer are here once.
//! - [`Flag`]: one `bool` key read strictly, where a section that does not
//!   decode is an error the reader logs and replaces by the default.
//! - [`strict`] and [`or_default`]: the two halves of that strict read, for
//!   the sections that are not a single flag.

use std::path::Path;

use jsonc_parser::cst::CstInputValue;
use serde::de::DeserializeOwned;

use super::error::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::keyed::{Checked, Keys};
use super::read_section;

/// The keys of a section to write, as `file::with_section` takes them.
pub(super) type Fields = Vec<(&'static str, CstInputValue)>;

/// A Settings section of `config.jsonc` read key by key: a bad value costs
/// only its own key (TUR-155).
pub(super) trait Section: Sized {
    /// The section's top-level key.
    const NAME: &'static str;

    /// The section from its keys, each bad or missing key read as its
    /// default (and recorded in `keys`).
    fn from_keys(keys: &mut Keys) -> Self;

    /// The keys to write: all of them, or only those that differ from
    /// `before`.
    fn fields(&self, before: Option<&Self>) -> Fields;

    /// The section from the text of `config.jsonc`, key by key, with what
    /// was not valid. Empty text, or no such key, is all defaults.
    fn read(raw: &str) -> Checked<Self> {
        let mut keys = Keys::read(raw, Self::NAME);
        let value = Self::from_keys(&mut keys);
        keys.checked(value)
    }

    /// [`Section::read`] without the problems (each one is logged).
    fn parse(raw: &str) -> Self {
        Self::read(raw).value
    }

    /// The section from `~/Meetings/.app/config.jsonc`, each bad key read
    /// as its default (logged). Read from disk every time, so an edit by
    /// hand needs no restart.
    fn current() -> Self {
        Self::parse(&super::raw_or_empty())
    }

    /// [`Section::current`], with what was not valid, for the Settings
    /// screen.
    fn current_checked() -> Checked<Self> {
        Self::read(&super::raw_or_empty())
    }

    /// `raw` with this section set to `value`, comments and other keys kept.
    #[cfg(test)]
    fn with(raw: &str, value: &Self) -> Result<String, ConfigError> {
        with_section(raw, Self::NAME, value.fields(None))
    }

    /// Save the section `merge` makes from the one on disk (each bad key
    /// read as its default), and return it as read back. Only the keys that
    /// differ from the file are written, so a bad key `merge` left alone
    /// stays as the user wrote it. `merge` runs under the write lock and may
    /// refuse.
    fn update<E: From<ConfigError>>(
        merge: impl FnOnce(&Self) -> Result<Self, E>,
    ) -> Result<Self, E> {
        let dir = super::app_dir().map_err(ConfigError::Root)?;
        Self::update_in(&dir, merge)
    }

    /// [`Section::update`] for the config folder `dir`.
    fn update_in<E: From<ConfigError>>(
        dir: &Path,
        merge: impl FnOnce(&Self) -> Result<Self, E>,
    ) -> Result<Self, E> {
        write_in(dir, |raw| -> Result<String, E> {
            let before = Self::parse(raw);
            let wanted = merge(&before)?;
            Ok(with_section(raw, Self::NAME, wanted.fields(Some(&before)))?)
        })?;
        Ok(Self::parse(&read_in(dir).map_err(E::from)?))
    }
}

/// The section `name` of `raw` decoded as `T`; `T`'s defaults when the file
/// is empty or has no such section. A file that does not parse, or a section
/// that does not decode, is [`ConfigError::Invalid`].
pub(super) fn strict<T: DeserializeOwned + Default>(
    raw: &str,
    name: &str,
) -> Result<T, ConfigError> {
    Ok(read_section(raw, name)
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default())
}

/// `read`'s value, or `default()` when it failed, logged as
/// "config.jsonc's `<why>`".
pub(super) fn or_default<T>(
    read: Result<T, ConfigError>,
    why: &str,
    default: impl FnOnce() -> T,
) -> T {
    read.unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's {why}");
        default()
    })
}

/// One `bool` key of a section, read strictly: the section is decoded as `R`
/// and [`Flag::pick`] takes the key from it.
pub(super) struct Flag<R> {
    /// The section's top-level key.
    pub(super) section: &'static str,
    /// The key inside it.
    pub(super) key: &'static str,
    /// The value when the key is missing, or the section is not valid.
    pub(super) default: bool,
    /// The key from the decoded section.
    pub(super) pick: fn(R) -> Option<bool>,
    /// What the log line says when the section is not valid, after
    /// "config.jsonc's".
    pub(super) when_invalid: &'static str,
}

impl<R: DeserializeOwned + Default> Flag<R> {
    /// The key from the text of `config.jsonc`; missing is the default.
    pub(super) fn parse(&self, raw: &str) -> Result<bool, ConfigError> {
        Ok((self.pick)(strict(raw, self.section)?).unwrap_or(self.default))
    }

    /// [`Flag::parse`], the default when the section is not valid (logged).
    pub(super) fn or_default(&self, raw: &str) -> bool {
        or_default(self.parse(raw), self.when_invalid, || self.default)
    }

    /// The key from `~/Meetings/.app/config.jsonc`, the default when it is
    /// missing, unreadable or not valid.
    pub(super) fn get(&self) -> bool {
        self.or_default(&super::raw_or_empty())
    }

    /// `raw` with the key set to `on`, comments and other keys kept.
    pub(super) fn with(&self, raw: &str, on: bool) -> Result<String, ConfigError> {
        with_section(raw, self.section, vec![(self.key, on.into())])
    }

    /// Save the key into `~/Meetings/.app/config.jsonc`, keeping everything
    /// else, and return it as read back.
    pub(super) fn set(&self, on: bool) -> Result<bool, ConfigError> {
        let dir = super::app_dir().map_err(ConfigError::Root)?;
        write_in(&dir, |raw| self.with(raw, on))?;
        self.parse(&read_in(&dir)?)
    }
}
