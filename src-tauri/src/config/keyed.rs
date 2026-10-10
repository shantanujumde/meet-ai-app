//! Reading one section of `config.jsonc` key by key (TUR-155), the one rule
//! every Settings section follows.
//!
//! A bad value (a wrong type, a word the app does not know, a number out of
//! range) costs only its own key: that key reads as its default, a warning
//! is logged, and every good key beside it is kept. A section that is not an
//! object, or a file that does not parse, reads as all defaults the same way.
//! The problems are also kept, so a Settings screen can say "this setting in
//! config.jsonc was not valid and is shown as the default" ([`Checked`]).
//!
//! Setters never refuse because of a key the user did not touch: they write
//! only the keys that changed, so a bad key they did not change stays on disk
//! as the user wrote it. The one refusal left is a file that does not parse
//! at all (`file::with_section`), which is never overwritten.

use std::fmt::Display;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// A section's values and, when some key was not valid, why: what a Settings
/// screen reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked<T> {
    pub value: T,
    /// Every bad value in the section, in one sentence-like line; `None` when
    /// all of it was valid (or missing).
    pub problem: Option<String>,
}

/// One section of `config.jsonc`, ready to be read key by key.
pub(super) struct Keys {
    section: &'static str,
    keys: Map<String, Value>,
    problems: Vec<String>,
}

impl Keys {
    /// The section `section` of the text `raw`. Never fails: a file that does
    /// not parse, or a section that is not an object, is no keys and one
    /// problem.
    pub(super) fn read(raw: &str, section: &'static str) -> Self {
        let mut keys = Self {
            section,
            keys: Map::new(),
            problems: Vec::new(),
        };
        match jsonc_parser::parse_to_serde_value::<Option<Value>>(raw, &Default::default()) {
            Err(error) => keys.problem(format!("config.jsonc does not parse: {error}")),
            Ok(file) => match file.as_ref().and_then(|file| file.get(section)) {
                None => {}
                Some(Value::Object(found)) => keys.keys = found.clone(),
                Some(other) => keys.problem(format!("{section} must be an object, not {other}")),
            },
        }
        keys
    }

    /// `key` as written, or `None` when it is missing or `null`.
    pub(super) fn raw(&self, key: &str) -> Option<&Value> {
        self.keys.get(key).filter(|value| !value.is_null())
    }

    /// `key` decoded as `T`. `None` when it is missing or `null`, and when it
    /// does not decode: then it is logged and kept as a problem.
    pub(super) fn get<T: DeserializeOwned>(&mut self, key: &str) -> Option<T> {
        let value = self.raw(key)?.clone();
        match T::deserialize(&value) {
            Ok(decoded) => Some(decoded),
            Err(error) => {
                self.bad(key, format!("{value} is not valid ({error})"));
                None
            }
        }
    }

    /// Record that `key` was not valid, and why: logged, and kept for
    /// [`Keys::checked`].
    pub(super) fn bad(&mut self, key: &str, why: impl Display) {
        self.problem(format!("{}.{key} {why}", self.section));
    }

    fn problem(&mut self, problem: String) {
        tracing::warn!(%problem, "config.jsonc has a bad value; using its default");
        self.problems.push(problem);
    }

    /// `value`, with the problems found reading it.
    pub(super) fn checked<T>(self, value: T) -> Checked<T> {
        let problem = (!self.problems.is_empty()).then(|| self.problems.join("; "));
        Checked { value, problem }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_key_costs_only_itself() {
        let mut keys = Keys::read(r#"{ "s": { "a": 3, "b": true, "c": null } }"#, "s");
        assert_eq!(keys.get::<bool>("a"), None);
        assert_eq!(keys.get::<bool>("b"), Some(true));
        assert_eq!(keys.get::<bool>("c"), None, "null is missing, not bad");
        assert_eq!(keys.get::<bool>("d"), None);
        let checked = keys.checked(());
        let problem = checked.problem.unwrap();
        assert!(problem.starts_with("s.a 3 is not valid"), "{problem}");
        assert!(!problem.contains("s.b"), "{problem}");
    }

    #[test]
    fn a_section_that_is_not_an_object_or_a_broken_file_is_no_keys_and_a_problem() {
        for raw in [r#"{ "s": null }"#, r#"{ "s": 3 }"#, "{ not json"] {
            let mut keys = Keys::read(raw, "s");
            assert_eq!(keys.get::<bool>("a"), None, "{raw}");
            assert!(keys.checked(()).problem.is_some(), "{raw}");
        }
        for raw in ["", "{}", r#"{ "other": 1 }"#] {
            assert_eq!(Keys::read(raw, "s").checked(()).problem, None, "{raw}");
        }
    }
}
