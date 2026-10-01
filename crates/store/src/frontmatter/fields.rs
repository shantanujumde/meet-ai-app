//! The ordered mapping type and its typed accessors.

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

/// An ordered frontmatter mapping. Key order and every key this crate has
/// never heard of are preserved across [`parse`] → [`render`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    pub(super) map: Hash,
}

/// A markdown file split into its frontmatter and everything after it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub frontmatter: Frontmatter,
    /// Everything after the closing `---` line, verbatim.
    pub body: String,
}

impl Frontmatter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The raw value under `key`, if any.
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        self.map.get(&Yaml::String(key.to_owned()))
    }

    /// `key` as a string. Integers, floats and booleans are returned in their
    /// written form, so `estimate: 2` reads as `"2"`. `null` and `~` read as
    /// `None`.
    ///
    /// Maps and lists also read as `None`. Integers and booleans come back in
    /// their normalised spelling (`0x1F` → `"31"`, `True` → `"true"`); floats
    /// exactly as written.
    pub fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(scalar_to_string)
    }

    /// `key` as an integer, if it is one.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Yaml::as_i64)
    }

    /// `key` as a list of strings. Accepts both `[a, b]` and block lists.
    /// Non-string items are rendered as strings; a scalar reads as a
    /// one-element list.
    ///
    /// `null` items and nested maps or lists inside the list are skipped. A
    /// `null` or map value reads as `None`.
    pub fn get_str_list(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key)? {
            Yaml::Array(items) => Some(items.iter().filter_map(scalar_to_string).collect()),
            other => scalar_to_string(other).map(|s| vec![s]),
        }
    }

    /// Set `key`. An existing key keeps its position; a new key is appended.
    pub fn set(&mut self, key: &str, value: Yaml) {
        // `replace`, not `insert`: hashlink's `insert` moves an existing entry
        // to the back, which would reorder the user's file on every status
        // change.
        self.map
            .replace(Yaml::String(key.to_owned()), normalize(value));
    }

    /// Set `key` to a string, or to `null` when `value` is `None`.
    pub fn set_str(&mut self, key: &str, value: Option<&str>) {
        let value = value.map_or(Yaml::Null, |s| Yaml::String(s.to_owned()));
        self.set(key, value);
    }

    /// Remove `key`, returning what was there.
    pub fn remove(&mut self, key: &str) -> Option<Yaml> {
        self.map.remove(&Yaml::String(key.to_owned()))
    }

    /// Every top-level key that is a string, in file order.
    pub fn keys(&self) -> Vec<String> {
        self.map
            .keys()
            .filter_map(|k| k.as_str().map(str::to_owned))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Replace the two variants the emitter cannot write back faithfully with
/// `Null`, so `parse(render(x)) == x` holds.
///
/// `BadValue` is what the loader produces for a value it rejected (`!!int
/// abc`); `Alias` never comes out of the loader but can be passed to
/// [`Frontmatter::set`]. The emitter writes the first as `~` and the second
/// as nothing at all.
pub(super) fn normalize(value: Yaml) -> Yaml {
    match value {
        Yaml::BadValue | Yaml::Alias(_) => Yaml::Null,
        Yaml::Array(items) => Yaml::Array(items.into_iter().map(normalize).collect()),
        Yaml::Hash(map) => Yaml::Hash(normalize_hash(map)),
        other => other,
    }
}

pub(super) fn normalize_hash(map: Hash) -> Hash {
    map.into_iter()
        .map(|(k, v)| (normalize(k), normalize(v)))
        .collect()
}

fn scalar_to_string(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}
