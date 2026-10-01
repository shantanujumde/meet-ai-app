//! Rendering a document back to text.

use std::fmt::Write as _;

use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter};

use super::fields::Document;
use super::parse::load_mapping;

/// Render a document back to text: `---\n<yaml>\n---\n<body>`.
///
/// The body is written verbatim. Rendering the output of [`parse`] and parsing
/// it again yields an equal [`Document`].
///
/// An empty mapping renders as `---\n---\n<body>`, not `---\n{}\n---`.
pub fn render(doc: &Document) -> String {
    format!(
        "---\n{}---\n{}",
        render_yaml(&doc.frontmatter.map),
        doc.body
    )
}

/// The YAML between the fences, ending in `\n`, or `""` for an empty mapping.
///
/// `YamlEmitter` is used first because its output is what a person expects to
/// read. Its quoting rule misses two spellings the loader then reads as
/// numbers — `"0o17"` and `"+.inf"` — so the output is re-parsed, and when it
/// does not come back equal the block is written again with every string
/// double-quoted. That costs one extra parse of a few hundred bytes per write,
/// and means a string can never silently turn into a number.
fn render_yaml(map: &Hash) -> String {
    if map.is_empty() {
        return String::new();
    }
    if let Some(yaml) = emit_with_yaml_rust2(map)
        && load_mapping(&yaml).as_ref() == Ok(map)
    {
        return yaml;
    }
    emit_quoted(map)
}

fn emit_with_yaml_rust2(map: &Hash) -> Option<String> {
    let mut out = String::new();
    YamlEmitter::new(&mut out)
        .dump(&Yaml::Hash(map.clone()))
        .ok()?;
    // `dump` opens with its own `---` document marker; ours is written by
    // `render`.
    let mut yaml = out.strip_prefix("---\n")?.to_owned();
    yaml.push('\n');
    Some(yaml)
}

/// Fallback emitter: one `key: value` line per top-level entry, every value
/// in flow style (`[...]`, `{...}`) and every string double-quoted.
pub(super) fn emit_quoted(map: &Hash) -> String {
    let mut out = String::new();
    for (key, value) in map {
        write_flow(&mut out, key);
        out.push_str(": ");
        write_flow(&mut out, value);
        out.push('\n');
    }
    out
}

fn write_flow(out: &mut String, value: &Yaml) {
    match value {
        Yaml::String(s) => write_quoted(out, s),
        Yaml::Real(s) => out.push_str(s),
        Yaml::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        Yaml::Boolean(b) => {
            let _ = write!(out, "{b}");
        }
        Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => out.push_str("null"),
        Yaml::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_flow(out, item);
            }
            out.push(']');
        }
        Yaml::Hash(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_flow(out, k);
                out.push_str(": ");
                write_flow(out, v);
            }
            out.push('}');
        }
    }
}

/// A YAML double-quoted scalar. Control characters and the characters YAML
/// treats as line breaks or a BOM are escaped; everything else is literal.
fn write_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
