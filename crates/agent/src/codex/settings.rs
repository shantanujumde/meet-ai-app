//! The model Codex runs when meet-ai passes no `--model` (TUR-131).
//!
//! Codex reads it from the top-level `model = "..."` key of `config.toml`
//! in its home folder (`$CODEX_HOME`, else `~/.codex`). This is a small line
//! reader for that one key, not a TOML parser: only lines before the first
//! `[table]` header count, and only a quoted string value. Anything else is
//! `None`, and Default then stays blank. The file is only read.

use std::path::{Path, PathBuf};

/// Codex's config file in its home folder.
const CONFIG_FILE: &str = "config.toml";

/// Largest config file read. A real one is a few KB.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Codex's home folder: `$CODEX_HOME`, else `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => dirs::home_dir().map(|home| home.join(".codex")),
    }
}

/// The model Codex picks on its own, as far as its user config says.
/// `None` when there is no such file or key; Codex then uses its built-in
/// default, which it writes nowhere.
pub fn settings_model() -> Option<String> {
    settings_model_in(&codex_home()?)
}

/// [`settings_model`] for the Codex home folder `dir`. A missing,
/// unreadable or oversized file is `None`.
pub fn settings_model_in(dir: &Path) -> Option<String> {
    let path = dir.join(CONFIG_FILE);
    if std::fs::metadata(&path).ok()?.len() > MAX_CONFIG_BYTES {
        return None;
    }
    top_level_model(&std::fs::read_to_string(&path).ok()?)
}

/// The value of the top-level `model` key in TOML `text`: lines before the
/// first `[table]` header, a basic (`"..."`) or literal (`'...'`) string.
/// Blank, escaped or unquoted values are `None`.
fn top_level_model(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "model" {
            continue;
        }
        return quoted(value.trim()).filter(|model| !model.is_empty());
    }
    None
}

/// The text inside a one-line quoted TOML string, with anything after the
/// closing quote allowed only as a `#` comment.
fn quoted(value: &str) -> Option<String> {
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &value[1..];
    let end = rest.find(quote)?;
    let inner = &rest[..end];
    let after = rest[end + 1..].trim();
    if !(after.is_empty() || after.starts_with('#')) {
        return None;
    }
    // An escape in a basic string would need real TOML parsing; no model
    // name has one.
    if quote == '"' && inner.contains('\\') {
        return None;
    }
    Some(inner.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_top_level_model() {
        let text = "# mine\napproval_policy = \"never\"\nmodel = \"gpt-5.6-terra\"\n\n[profiles.fast]\nmodel = \"gpt-5.6-luna\"\n";
        assert_eq!(top_level_model(text).as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(
            top_level_model("  model='gpt-5.5'  # pinned").as_deref(),
            Some("gpt-5.5")
        );
    }

    #[test]
    fn a_model_only_inside_a_table_does_not_count() {
        let text = "[profiles.fast]\nmodel = \"gpt-5.6-luna\"\n";
        assert_eq!(top_level_model(text), None);
    }

    #[test]
    fn other_keys_and_bad_values_are_none() {
        for text in [
            "",
            "model_provider = \"openai\"",
            "model_reasoning_effort = \"high\"",
            "model = \"\"",
            "model = gpt-5.5",
            "model = \"gpt-5.5\" extra",
            "model = \"unterminated",
            "model = \"a\\\"b\"",
        ] {
            assert_eq!(top_level_model(text), None, "{text}");
        }
    }

    #[test]
    fn reads_the_file_in_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(settings_model_in(dir.path()), None);
        std::fs::write(dir.path().join(CONFIG_FILE), "model = \"gpt-5.6-sol\"\n").unwrap();
        assert_eq!(
            settings_model_in(dir.path()).as_deref(),
            Some("gpt-5.6-sol")
        );
    }
}
