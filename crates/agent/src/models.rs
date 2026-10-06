//! The models each harness offers the setup picker (TUR-74).
//!
//! Claude Code has no way to list its models without starting a session, so
//! its list is data: `crates/agent/models.json`, next to this crate's code,
//! so adding a model is a data edit. Codex lists its own (`codex debug
//! models`, [`crate::CodexHarness`]); its entries in the file are what the
//! picker shows when Codex cannot be asked, and where labels and notes come
//! from.
//!
//! Order matters. The setup screen offers "Default" (no `--model`, the CLI
//! picks) and the first two models as buttons, and the rest in a dropdown.
//! Free text stays allowed: new models ship faster than this list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// The per-harness lists, keyed by harness id (`claude-code`, `codex`).
const CATALOG: &str = include_str!("../models.json");

/// How long a model list a CLI gave stays good. Codex's list changes with a
/// Codex update, not between two opens of Settings.
pub const LIST_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// One model the picker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// What `--model` gets: an alias (`sonnet`) or a full id.
    pub name: String,
    /// What the screen calls it: `Sonnet`. The name itself when the list
    /// has no label for it.
    pub label: String,
    /// One line on when to pick it ("fastest, cheapest"), if the list has one.
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    name: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

impl From<Entry> for Model {
    fn from(entry: Entry) -> Self {
        Self {
            label: entry.label.unwrap_or_else(|| entry.name.clone()),
            name: entry.name,
            note: entry.note,
        }
    }
}

fn catalog() -> &'static HashMap<String, Vec<Model>> {
    static PARSED: OnceLock<HashMap<String, Vec<Model>>> = OnceLock::new();
    PARSED.get_or_init(|| {
        // A unit test parses the same text, so this only fails in a build
        // whose models.json was never tested.
        match serde_json::from_str::<HashMap<String, Vec<Entry>>>(CATALOG) {
            Ok(lists) => lists
                .into_iter()
                .map(|(id, entries)| (id, entries.into_iter().map(Model::from).collect()))
                .collect(),
            Err(e) => {
                tracing::warn!("models.json is not valid: {e}");
                HashMap::new()
            }
        }
    })
}

/// The listed models for `harness` (an id like `claude-code`), in order.
/// Empty for a harness the file does not name.
pub fn listed(harness: &str) -> Vec<Model> {
    catalog().get(harness).cloned().unwrap_or_default()
}

/// `names` as the picker shows them, in the given order: label and note from
/// the file where it has the name, the bare name otherwise. Duplicates and
/// blank names are dropped.
pub fn described(harness: &str, names: impl IntoIterator<Item = String>) -> Vec<Model> {
    described_models(
        harness,
        names.into_iter().map(|name| Model {
            label: name.clone(),
            name,
            note: None,
        }),
    )
}

/// [`described`] for models the CLI already labelled (Codex's
/// `display_name` and `description`): the file's label and note win where it
/// has them, the CLI's stand in where it does not.
pub fn described_models(harness: &str, models: impl IntoIterator<Item = Model>) -> Vec<Model> {
    let known = catalog().get(harness);
    let mut out: Vec<Model> = Vec::new();
    for model in models {
        let name = model.name.trim().to_owned();
        if name.is_empty() || out.iter().any(|m| m.name == name) {
            continue;
        }
        let listed = known.and_then(|list| list.iter().find(|m| m.name == name));
        // A file entry without a label has its name as label; that is no label.
        let label = listed
            .map(|m| m.label.clone())
            .filter(|label| *label != name)
            .or_else(|| Some(model.label.trim().to_owned()).filter(|l| !l.is_empty()))
            .unwrap_or_else(|| name.clone());
        let note = listed.and_then(|m| m.note.clone()).or(model.note);
        out.push(Model { name, label, note });
    }
    out
}

/// Model lists a CLI gave, one per binary, kept for a while so opening
/// Settings does not start the CLI every time. Only a non-empty list is
/// kept: an empty one means the CLI could not be asked, and the next open
/// asks again.
#[derive(Debug)]
pub struct ListCache {
    ttl: Duration,
    lists: Mutex<HashMap<PathBuf, (Instant, Vec<Model>)>>,
}

impl ListCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            lists: Mutex::new(HashMap::new()),
        }
    }

    /// The list kept for `binary` if it is younger than the time limit,
    /// otherwise what `list` returns now. The lock is not held while `list`
    /// runs.
    pub fn get_or_list(&self, binary: &Path, list: impl FnOnce() -> Vec<Model>) -> Vec<Model> {
        {
            let lists = self.lists.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((at, models)) = lists.get(binary)
                && at.elapsed() < self.ttl
            {
                return models.clone();
            }
        }
        let models = list();
        if !models.is_empty() {
            self.lists
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(binary.to_owned(), (Instant::now(), models.clone()));
        }
        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_parses_and_every_harness_has_its_list() {
        let lists: HashMap<String, Vec<Entry>> = serde_json::from_str(CATALOG).unwrap();
        assert_eq!(lists.len(), 2, "{:?}", lists.keys());
        for id in [crate::claude::ID, crate::codex::ID] {
            assert!(!listed(id).is_empty(), "{id}");
        }
    }

    #[test]
    fn claude_code_starts_with_sonnet_and_haiku_then_opus() {
        let names: Vec<String> = listed(crate::claude::ID)
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(&names[..3], ["sonnet", "haiku", "opus"]);
        for id in [
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-haiku-4-5",
            "claude-fable-5-1",
        ] {
            assert!(names.iter().any(|name| name == id), "{id} missing");
        }
    }

    #[test]
    fn no_name_could_be_read_as_a_flag_and_none_repeats() {
        for id in [crate::claude::ID, crate::codex::ID] {
            let list = listed(id);
            for (i, model) in list.iter().enumerate() {
                assert!(!model.name.trim().is_empty(), "{id}: blank name");
                assert!(!model.name.starts_with('-'), "{id}: {}", model.name);
                assert!(
                    !list[..i].iter().any(|other| other.name == model.name),
                    "{id}: {} twice",
                    model.name
                );
            }
        }
    }

    #[test]
    fn labels_fall_back_to_the_name() {
        let haiku = &listed(crate::claude::ID)[1];
        assert_eq!(haiku.label, "Haiku");
        assert_eq!(haiku.note.as_deref(), Some("fastest, cheapest"));
        let full = described(crate::claude::ID, ["claude-haiku-4-5".to_owned()]);
        assert_eq!(full[0].label, "claude-haiku-4-5");
        assert_eq!(full[0].note, None);
    }

    #[test]
    fn described_keeps_order_and_drops_blanks_and_repeats() {
        let names = ["gpt-x", " ", "sonnet", "gpt-x"].map(str::to_owned);
        let models = described(crate::claude::ID, names);
        let got: Vec<_> = models.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(got, ["gpt-x", "Sonnet"]);
    }

    #[test]
    fn the_file_s_label_and_note_win_over_the_cli_s() {
        let cli = |name: &str, label: &str, note: Option<&str>| Model {
            name: name.into(),
            label: label.into(),
            note: note.map(Into::into),
        };
        let models = described_models(
            crate::claude::ID,
            [
                cli("haiku", "Claude Haiku", Some("from the CLI")),
                cli("gpt-new", "GPT New", Some("Codex's note")),
                cli("claude-opus-5-5", "Opus 5.5", None),
                cli("bare", " ", None),
            ],
        );
        assert_eq!(models[0], cli("haiku", "Haiku", Some("fastest, cheapest")));
        assert_eq!(models[1], cli("gpt-new", "GPT New", Some("Codex's note")));
        assert_eq!(models[2], cli("claude-opus-5-5", "Opus 5.5", None));
        assert_eq!(models[3], cli("bare", "bare", None));
    }

    fn named(names: &[&str]) -> Vec<Model> {
        described(crate::codex::ID, names.iter().map(|n| (*n).to_owned()))
    }

    #[test]
    fn the_cache_keeps_a_list_until_it_is_too_old() {
        let cache = ListCache::new(Duration::from_secs(60));
        let bin = Path::new("/bin/codex");
        assert_eq!(cache.get_or_list(bin, || named(&["a"])), named(&["a"]));
        assert_eq!(cache.get_or_list(bin, || named(&["b"])), named(&["a"]));
        assert_eq!(
            cache.get_or_list(Path::new("/other"), || named(&["c"])),
            named(&["c"])
        );

        let stale = ListCache::new(Duration::ZERO);
        assert_eq!(stale.get_or_list(bin, || named(&["a"])), named(&["a"]));
        assert_eq!(stale.get_or_list(bin, || named(&["b"])), named(&["b"]));
    }

    #[test]
    fn the_cache_never_keeps_an_empty_list() {
        let cache = ListCache::new(Duration::from_secs(60));
        let bin = Path::new("/bin/codex");
        assert!(cache.get_or_list(bin, Vec::new).is_empty());
        assert_eq!(cache.get_or_list(bin, || named(&["a"])), named(&["a"]));
    }
}
