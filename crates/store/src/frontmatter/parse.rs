//! Splitting a file into frontmatter and body, and loading the YAML safely.

use yaml_rust2::parser::Parser;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Event, Yaml, YamlLoader};

use super::fields::{Document, Frontmatter, normalize_hash};
use crate::Problem;

/// Split a file into `(frontmatter_yaml, body)`.
///
/// The file must start with a line that is exactly `---` (a UTF-8 BOM and
/// `\r\n` line endings are tolerated). The block ends at the next line that is
/// exactly `---`. Returns `None` for the YAML when there is no opening line or
/// no closing line — in that case the whole input is the body.
pub fn split(raw: &str) -> (Option<&str>, &str) {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut lines = text.split_inclusive('\n');
    let Some(opening) = lines.next().filter(|line| is_fence(line)) else {
        return (None, raw);
    };
    let rest = &text[opening.len()..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if is_fence(line) {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, raw)
}

/// Parse a whole file.
///
/// * No frontmatter block → `Err(Problem::NoFrontmatter)`.
/// * Block present but not valid YAML, or not a mapping (an empty block is an
///   empty mapping, not an error) → `Err(Problem::BadFrontmatter)`.
///
/// Callers decide what a missing block means for their file type.
pub fn parse(raw: &str) -> Result<Document, Problem> {
    let (yaml, body) = split(raw);
    let yaml = yaml.ok_or(Problem::NoFrontmatter)?;
    let map = load_mapping(yaml).map_err(|detail| Problem::BadFrontmatter { detail })?;
    Ok(Document {
        frontmatter: Frontmatter { map },
        body: body.to_owned(),
    })
}

/// Is `line` (with or without its line ending) exactly `---`?
fn is_fence(line: &str) -> bool {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    line == "---"
}

/// How deeply maps and lists may nest in a frontmatter block.
///
/// Real frontmatter is two or three levels deep. The cap exists because
/// `YamlLoader` recurses once per level: a 2 KB block of `- - - - …` overflows
/// the stack, and a stack overflow aborts the process — every launch, until
/// someone finds and deletes the file. Past this depth the block is
/// `BadFrontmatter` instead.
const MAX_DEPTH: usize = 64;

/// Walk the block's event stream and refuse the two shapes that are dangerous
/// to hand to `YamlLoader`, before it builds anything.
///
/// The event parser keeps its own state on the heap, so this walk is safe at
/// any depth. The two refusals:
///
/// * nesting deeper than [`MAX_DEPTH`] (stack overflow, see above);
/// * any alias (`*name`). The loader copies the anchored value for every
///   alias, so ten aliases of ten aliases of … turn a few hundred bytes into
///   gigabytes. Frontmatter is flat metadata an agent writes; nothing in SPEC
///   §3.2 or §3.3 needs an alias, so refusing all of them is simpler and safer
///   than budgeting their expansion.
fn check_shape(yaml: &str) -> Result<(), String> {
    let mut parser = Parser::new_from_str(yaml);
    let mut depth = 0usize;
    loop {
        let (event, marker) = parser.next_token().map_err(|e| e.to_string())?;
        match event {
            Event::StreamEnd => return Ok(()),
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(format!(
                        "maps and lists nest more than {MAX_DEPTH} levels deep (line {})",
                        marker.line()
                    ));
                }
            }
            Event::SequenceEnd | Event::MappingEnd => depth = depth.saturating_sub(1),
            Event::Alias(_) => {
                return Err(format!(
                    "YAML aliases (`*name`) are not supported in frontmatter (line {})",
                    marker.line()
                ));
            }
            _ => {}
        }
    }
}

/// Load a frontmatter block as a mapping, or say why it is not one.
pub(super) fn load_mapping(yaml: &str) -> Result<Hash, String> {
    check_shape(yaml)?;
    let mut docs = YamlLoader::load_from_str(yaml).map_err(|e| e.to_string())?;
    if docs.len() > 1 {
        return Err("the block holds more than one YAML document".to_owned());
    }
    match docs.pop() {
        // A block that is empty or only comments loads as no document, or as
        // one empty (`BadValue`) document.
        None | Some(Yaml::BadValue) => Ok(Hash::new()),
        Some(Yaml::Hash(map)) => Ok(normalize_hash(map)),
        Some(other) => Err(format!(
            "expected a mapping of keys to values, found {}",
            kind(&other)
        )),
    }
}

fn kind(value: &Yaml) -> &'static str {
    match value {
        Yaml::Array(_) => "a list",
        Yaml::Hash(_) => "a mapping",
        Yaml::Null => "null",
        _ => "a single value",
    }
}
