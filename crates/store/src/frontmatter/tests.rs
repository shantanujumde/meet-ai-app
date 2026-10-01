use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use super::parse::load_mapping;
use super::write::emit_quoted;
use super::*;
use crate::Problem;

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
