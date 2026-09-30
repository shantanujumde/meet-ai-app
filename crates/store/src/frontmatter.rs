//! The `---` YAML block at the top of `meeting.md` and `TICK-NNNN.md`.
//!
//! SETUP.md §1.2: parse to an ordered `yaml-rust2` value, mutate only the keys
//! this code owns, emit. Unknown keys survive because they are never modelled
//! — there is deliberately no struct here to deserialize into.
//!
//! What survives a round trip is the *values*: every key, its position, and
//! nested maps and lists. What does not survive is the *spelling*, because
//! `yaml-rust2` keeps no record of it:
//!
//! * YAML comments inside the block are dropped (the scanner discards them),
//!   so the `# optional` notes in the SPEC §3.2 example vanish on first write.
//! * Quoting and list style are re-chosen on output: `[a, b]` comes back as a
//!   block list, and a value with `:` in it (timestamps, `00:14:22`) comes back
//!   double-quoted whether or not it was quoted before.
//! * Numbers and booleans are normalised: `0x1F` is written back as `31`,
//!   `True` as `true`. An anchor (`&name`) is dropped on write.
//!
//! Two shapes are refused as `BadFrontmatter` rather than loaded: any alias
//! (`*name`), and nesting deeper than 64 levels. Both are ways a tiny file can
//! take the whole app down — see `MAX_DEPTH` and `check_shape`.

use std::fmt::Write as _;

use yaml_rust2::parser::Parser;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Event, Yaml, YamlEmitter, YamlLoader};

use crate::Problem;

/// An ordered frontmatter mapping. Key order and every key this crate has
/// never heard of are preserved across [`parse`] → [`render`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    map: Hash,
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
fn load_mapping(yaml: &str) -> Result<Hash, String> {
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

/// Replace the two variants the emitter cannot write back faithfully with
/// `Null`, so `parse(render(x)) == x` holds.
///
/// `BadValue` is what the loader produces for a value it rejected (`!!int
/// abc`); `Alias` never comes out of the loader but can be passed to
/// [`Frontmatter::set`]. The emitter writes the first as `~` and the second
/// as nothing at all.
fn normalize(value: Yaml) -> Yaml {
    match value {
        Yaml::BadValue | Yaml::Alias(_) => Yaml::Null,
        Yaml::Array(items) => Yaml::Array(items.into_iter().map(normalize).collect()),
        Yaml::Hash(map) => Yaml::Hash(normalize_hash(map)),
        other => other,
    }
}

fn normalize_hash(map: Hash) -> Hash {
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
fn emit_quoted(map: &Hash) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(raw: &str) -> Frontmatter {
        parse(raw).expect("valid frontmatter").frontmatter
    }

    /// `parse(render(parse(raw)))` equals `parse(raw)`, and the rendered text.
    fn round_trip(raw: &str) -> (Document, String) {
        let first = parse(raw).expect("first parse");
        let text = render(&first);
        let second = parse(&text).expect("rendered output must parse");
        assert_eq!(first, second, "round trip changed the document:\n{text}");
        (second, text)
    }

    const SPEC_3_2: &str = "---
id: 2026-09-01-1430-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
duration_sec: 2714
attendees: [Shantanu, Priya, Dev]        # from EventKit when available
calendar_event_id: \"ABC123\"              # optional
repo: ~/apps/api                          # optional link
analyzed_by: claude-code                  # agent stamps this
analyzed_at: 2026-09-01T15:32:00+05:30
---

## Summary
## Decisions
## Action Items
## Open Questions
";

    #[test]
    fn the_spec_3_2_example_reads_as_expected() {
        let doc = parse(SPEC_3_2).unwrap();
        let fm = &doc.frontmatter;
        assert_eq!(fm.get_str("id").as_deref(), Some("2026-09-01-1430-standup"));
        assert_eq!(
            fm.get_str("date").as_deref(),
            Some("2026-09-01T14:30:00+05:30")
        );
        assert_eq!(fm.get_i64("duration_sec"), Some(2714));
        assert_eq!(
            fm.get_str_list("attendees"),
            Some(vec!["Shantanu".into(), "Priya".into(), "Dev".into()])
        );
        assert_eq!(fm.get_str("repo").as_deref(), Some("~/apps/api"));
        assert!(doc.body.starts_with("\n## Summary\n"));
    }

    #[test]
    fn the_spec_3_2_example_renders_as_snapshot() {
        let (_, text) = round_trip(SPEC_3_2);
        insta::assert_snapshot!(text);
    }

    #[test]
    fn unknown_keys_and_their_order_survive_a_round_trip() {
        let raw = "---\nzeta: 1\nid: TICK-0001\nagent_note: keep me\nalpha: [x, y]\n---\nbody\n";
        let (doc, _) = round_trip(raw);
        assert_eq!(
            doc.frontmatter.keys(),
            ["zeta", "id", "agent_note", "alpha"]
        );
        assert_eq!(
            doc.frontmatter.get_str("agent_note").as_deref(),
            Some("keep me")
        );
    }

    #[test]
    fn nested_maps_and_lists_survive_a_round_trip() {
        let raw = "---
links:
  linear: {id: ENG-1, url: \"https://linear.app/x/ENG-1\"}
  history:
    - {at: \"00:01:02\", who: [a, b]}
    - plain
    - [1, 2.50, true, null]
empty_list: []
empty_map: {}
---
";
        let (doc, _) = round_trip(raw);
        let links = doc.frontmatter.get("links").unwrap();
        assert_eq!(
            links["linear"]["url"].as_str(),
            Some("https://linear.app/x/ENG-1")
        );
        assert_eq!(links["history"][2][1], Yaml::Real("2.50".into()));
    }

    #[test]
    fn strings_that_look_like_other_types_stay_strings() {
        let looks_alike = [
            "00:14:22",
            "123",
            "-7",
            "+5",
            "1.5",
            "1e5",
            "null",
            "Null",
            "~",
            "true",
            "False",
            "yes",
            "0x1F",
            ".nan",
            "",
            " padded ",
            "2026-09-01",
            "a: b",
            "# not a comment",
            "[not, a, list]",
            "line\nbreak",
            "quote\"s",
            "---",
        ];
        let mut fm = Frontmatter::new();
        for (i, s) in looks_alike.iter().enumerate() {
            fm.set_str(&format!("k{i}"), Some(s));
        }
        let text = render(&Document {
            frontmatter: fm.clone(),
            body: String::new(),
        });
        // Bare keys mean `YamlEmitter` did the quoting, not the fallback.
        assert!(text.starts_with("---\nk0: \"00:14:22\"\n"), "{text}");
        let back = parse(&text).unwrap().frontmatter;
        assert_eq!(back, fm, "rendered as:\n{text}");
        for (i, s) in looks_alike.iter().enumerate() {
            assert_eq!(back.get(&format!("k{i}")), Some(&Yaml::String((*s).into())));
        }
    }

    #[test]
    fn quoted_look_alikes_from_a_file_survive_a_round_trip() {
        let raw = "---\ntranscript_ref: \"00:14:22\"\ncalendar_event_id: \"123\"\nsynced_to: \"null\"\nflag: \"true\"\n---\n";
        let (doc, text) = round_trip(raw);
        for key in ["transcript_ref", "calendar_event_id", "synced_to", "flag"] {
            assert!(
                matches!(doc.frontmatter.get(key), Some(Yaml::String(_))),
                "{key}: {text}"
            );
        }
        assert!(text.contains("transcript_ref: \"00:14:22\""), "{text}");
        assert!(text.contains("calendar_event_id: \"123\""), "{text}");
    }

    #[test]
    fn a_string_the_emitter_would_leave_bare_falls_back_to_quoting() {
        // yaml-rust2's emitter writes `0o17` and `+.inf` unquoted, and its
        // loader reads them back as the integer 15 and infinity.
        for odd in ["0o17", "+.inf"] {
            let mut fm = Frontmatter::new();
            fm.set_str("id", Some("TICK-0001"));
            fm.set_str("odd", Some(odd));
            fm.set(
                "nested",
                Yaml::Array(vec![Yaml::String(odd.into()), Yaml::Integer(1)]),
            );
            let doc = Document {
                frontmatter: fm,
                body: "body".into(),
            };
            let text = render(&doc);
            assert_eq!(
                text,
                format!(
                    "---\n\"id\": \"TICK-0001\"\n\"odd\": \"{odd}\"\n\"nested\": [\"{odd}\", 1]\n---\nbody"
                )
            );
            assert_eq!(parse(&text).unwrap(), doc);
        }
    }

    #[test]
    fn the_fallback_escapes_control_characters_and_quotes() {
        let tricky = "0o17 is not here; tab\there \"q\" back\\slash\nnew \u{1} \u{2028} é";
        let mut map = Hash::new();
        map.insert(Yaml::String("k".into()), Yaml::String(tricky.into()));
        let yaml = emit_quoted(&map);
        assert_eq!(load_mapping(&yaml), Ok(map), "{yaml}");
    }

    #[test]
    fn a_bom_and_crlf_line_endings_are_tolerated() {
        let raw = "\u{feff}---\r\nid: TICK-0001\r\nstatus: open\r\n---\r\nBody\r\nmore\r\n";
        let (yaml, body) = split(raw);
        assert_eq!(yaml, Some("id: TICK-0001\r\nstatus: open\r\n"));
        assert_eq!(body, "Body\r\nmore\r\n");
        let (doc, _) = round_trip(raw);
        assert_eq!(doc.frontmatter.get_str("status").as_deref(), Some("open"));
        assert_eq!(doc.body, "Body\r\nmore\r\n");
    }

    #[test]
    fn the_body_is_everything_after_the_closing_line_verbatim() {
        let raw = "---\na: 1\n---\n\n  indented\n---\ntrailing";
        let (yaml, body) = split(raw);
        assert_eq!(yaml, Some("a: 1\n"));
        assert_eq!(body, "\n  indented\n---\ntrailing");
        assert_eq!(split("---\na: 1\n---"), (Some("a: 1\n"), ""));
    }

    #[test]
    fn a_file_without_a_block_has_no_frontmatter() {
        for raw in [
            "",
            "# Title\n",
            "\n---\na: 1\n---\n",
            "--- \na: 1\n---\n",
            "----\n",
        ] {
            assert_eq!(split(raw), (None, raw), "{raw:?}");
            assert_eq!(parse(raw), Err(Problem::NoFrontmatter), "{raw:?}");
        }
    }

    #[test]
    fn an_unclosed_block_has_no_frontmatter() {
        let raw = "---\nid: TICK-0001\nno closing line\n";
        assert_eq!(split(raw), (None, raw));
        assert_eq!(parse(raw), Err(Problem::NoFrontmatter));
        assert_eq!(parse("---"), Err(Problem::NoFrontmatter));
    }

    #[test]
    fn invalid_yaml_is_bad_frontmatter() {
        for raw in [
            "---\nid: [unclosed\n---\n",
            "---\na: 1\na: 2\n---\n",
            "---\nkey: \"open quote\n---\n",
        ] {
            assert!(
                matches!(parse(raw), Err(Problem::BadFrontmatter { .. })),
                "{raw:?} -> {:?}",
                parse(raw)
            );
        }
    }

    #[test]
    fn a_list_or_scalar_instead_of_a_mapping_is_bad_frontmatter() {
        for raw in [
            "---\n- a\n- b\n---\n",
            "---\njust text\n---\n",
            "---\n42\n---\n",
            "---\nnull\n---\n",
        ] {
            assert!(
                matches!(parse(raw), Err(Problem::BadFrontmatter { .. })),
                "{raw:?} -> {:?}",
                parse(raw)
            );
        }
    }

    #[test]
    fn an_empty_block_is_an_empty_mapping() {
        for raw in [
            "---\n---\nbody",
            "---\n\n---\nbody",
            "---\n# only a comment\n---\nbody",
            "---\n{}\n---\nbody",
        ] {
            let doc = parse(raw).unwrap_or_else(|p| panic!("{raw:?}: {p:?}"));
            assert!(doc.frontmatter.is_empty(), "{raw:?}");
            assert_eq!(doc.body, "body");
        }
        let (_, text) = round_trip("---\n---\nbody");
        assert_eq!(text, "---\n---\nbody");
    }

    #[test]
    fn comments_inside_the_block_are_dropped() {
        // Documents a yaml-rust2 limitation the module doc relies on.
        let (_, text) = round_trip("---\n# leading\nstatus: open # trailing\n---\n");
        assert_eq!(text, "---\nstatus: open\n---\n");
    }

    #[test]
    fn set_keeps_an_existing_keys_position_and_appends_a_new_one() {
        let mut fm = fm("---\nid: TICK-0001\nstatus: open\nassignee: Shantanu\n---\n");
        fm.set_str("status", Some("done"));
        fm.set("estimate", Yaml::String("2d".into()));
        assert_eq!(fm.keys(), ["id", "status", "assignee", "estimate"]);
        assert_eq!(fm.get_str("status").as_deref(), Some("done"));
        let text = render(&Document {
            frontmatter: fm,
            body: String::new(),
        });
        assert_eq!(
            text,
            "---\nid: TICK-0001\nstatus: done\nassignee: Shantanu\nestimate: 2d\n---\n"
        );
    }

    #[test]
    fn set_str_none_writes_null() {
        let mut fm = fm("---\nsynced_to: linear\n---\n");
        fm.set_str("synced_to", None);
        fm.set_str("external_id", None);
        assert_eq!(fm.get("synced_to"), Some(&Yaml::Null));
        assert_eq!(fm.get_str("synced_to"), None);
        let text = render(&Document {
            frontmatter: fm,
            body: String::new(),
        });
        assert_eq!(text, "---\nsynced_to: ~\nexternal_id: ~\n---\n");
        let back = parse(&text).unwrap().frontmatter;
        assert_eq!(back.get("external_id"), Some(&Yaml::Null));
    }

    #[test]
    fn remove_returns_the_old_value_and_keeps_the_rest_in_order() {
        let mut fm = fm("---\na: 1\nb: 2\nc: 3\n---\n");
        assert_eq!(fm.remove("b"), Some(Yaml::Integer(2)));
        assert_eq!(fm.remove("b"), None);
        assert_eq!(fm.keys(), ["a", "c"]);
    }

    #[test]
    fn get_str_reads_scalars_in_their_written_form() {
        let fm = fm("---
estimate: 2
ratio: 1.50
done: true
nothing: null
tilde: ~
list: [a]
map: {a: 1}
text: hi
---
");
        assert_eq!(fm.get_str("estimate").as_deref(), Some("2"));
        assert_eq!(fm.get_str("ratio").as_deref(), Some("1.50"));
        assert_eq!(fm.get_str("done").as_deref(), Some("true"));
        assert_eq!(fm.get_str("text").as_deref(), Some("hi"));
        for key in ["nothing", "tilde", "list", "map", "missing"] {
            assert_eq!(fm.get_str(key), None, "{key}");
        }
        assert_eq!(fm.get_i64("estimate"), Some(2));
        assert_eq!(fm.get_i64("ratio"), None);
        assert_eq!(fm.get_i64("text"), None);
    }

    #[test]
    fn get_str_list_reads_flow_block_and_scalar_forms() {
        let fm = fm("---
flow: [a, b]
block:
  - a
  - 2
  - null
  - true
scalar: solo
nothing: null
map: {a: 1}
---
");
        assert_eq!(fm.get_str_list("flow"), Some(vec!["a".into(), "b".into()]));
        assert_eq!(
            fm.get_str_list("block"),
            Some(vec!["a".into(), "2".into(), "true".into()])
        );
        assert_eq!(fm.get_str_list("scalar"), Some(vec!["solo".into()]));
        assert_eq!(fm.get_str_list("nothing"), None);
        assert_eq!(fm.get_str_list("map"), None);
        assert_eq!(fm.get_str_list("missing"), None);
    }

    #[test]
    fn a_rejected_tagged_value_round_trips_as_null() {
        let (doc, _) = round_trip("---\nn: !!int abc\n---\n");
        assert_eq!(doc.frontmatter.get("n"), Some(&Yaml::Null));
    }
}
