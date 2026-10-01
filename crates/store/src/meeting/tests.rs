use super::*;

const SPEC_EXAMPLE: &str = "---
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

/// A well-formed file with content in every section.
const FULL: &str = "---
id: 2026-09-01-1430-standup
title: Platform Standup
---

## Summary
We talked about sessions.

### Detail
Redis, probably.

## Decisions
- Move sessions to Redis.

## Action Items
- TICK-0001

## Open Questions
- Who owns the migration?
";

fn temp_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("meet-ai-store-meeting-{name}-"))
        .tempdir()
        .unwrap()
}

fn body_of(raw: &str) -> String {
    frontmatter::parse(raw).expect("valid frontmatter").body
}

#[test]
fn the_spec_example_parses_cleanly() {
    let m = Meeting::parse(SPEC_EXAMPLE);
    assert_eq!(m.problems, vec![]);
    assert_eq!(m.id().as_deref(), Some("2026-09-01-1430-standup"));
    assert_eq!(m.title().as_deref(), Some("Platform Standup"));
    assert_eq!(m.date().as_deref(), Some("2026-09-01T14:30:00+05:30"));
    assert_eq!(m.duration_sec(), Some(2714));
    assert_eq!(m.attendees(), vec!["Shantanu", "Priya", "Dev"]);
    assert!(m.is_analyzed());
    let headings: Vec<_> = m.sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(headings, SECTIONS);
    assert_eq!(m.section("Summary"), Some(""));
}

#[test]
fn unknown_frontmatter_keys_survive_a_round_trip() {
    let raw = "---
id: m1
title: T
mood: optimistic
agent_notes:
  confidence: 0.8
  sources: [transcript, notes]
---

## Summary
## Decisions
## Action Items
## Open Questions
";
    let m = Meeting::parse(raw);
    assert_eq!(m.problems, vec![]);
    let again = Meeting::parse(&m.render().unwrap());
    assert_eq!(again.frontmatter, m.frontmatter);
    assert_eq!(
        again.frontmatter.keys(),
        vec!["id", "title", "mood", "agent_notes"]
    );
    assert_eq!(
        again.frontmatter.get_str("mood").as_deref(),
        Some("optimistic")
    );
}

#[test]
fn an_unknown_section_survives_in_position() {
    let raw = FULL.replace("## Decisions", "## Risks\nNone yet.\n\n## Decisions");
    let m = Meeting::parse(&raw);
    assert_eq!(m.problems, vec![]);
    let headings: Vec<_> = m.sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(
        headings,
        [
            "Summary",
            "Risks",
            "Decisions",
            "Action Items",
            "Open Questions"
        ]
    );
    let rendered = m.render().unwrap();
    assert_eq!(body_of(&rendered), body_of(&raw));
    assert_eq!(
        Meeting::parse(&rendered).section("risks"),
        Some("None yet.\n\n")
    );
}

#[test]
fn reordered_sections_are_still_found() {
    let raw = "---
id: m1
title: T
---
## Open Questions
q
## Action Items
a
## Summary
s
## Decisions
d
";
    let m = Meeting::parse(raw);
    assert_eq!(m.problems, vec![]);
    assert_eq!(m.section("Summary"), Some("s\n"));
    assert_eq!(m.section("Decisions"), Some("d\n"));
    assert_eq!(m.section("Action Items"), Some("a\n"));
    assert_eq!(m.section("Open Questions"), Some("q\n"));
}

#[test]
fn sections_match_case_insensitively_on_trimmed_text() {
    let m = Meeting::parse("---\nid: m\ntitle: t\n---\n##   action items  \nx\n");
    assert_eq!(m.sections[0].heading, "action items");
    assert_eq!(m.section("Action Items"), Some("x\n"));
    assert_eq!(m.section("  ACTION ITEMS "), Some("x\n"));
}

#[test]
fn a_missing_section_is_flagged_and_the_rest_still_load() {
    let raw = FULL.replace("## Decisions\n- Move sessions to Redis.\n\n", "");
    let m = Meeting::parse(&raw);
    assert_eq!(
        m.problems,
        vec![Problem::MissingSection {
            heading: "Decisions".into()
        }]
    );
    assert_eq!(m.section("Decisions"), None);
    assert_eq!(m.section("Action Items"), Some("- TICK-0001\n\n"));
    assert_eq!(m.id().as_deref(), Some("2026-09-01-1430-standup"));
}

#[test]
fn missing_id_and_title_are_flagged() {
    let m = Meeting::parse("---\ndate: 2026-09-01\n---\n");
    assert!(
        m.problems
            .contains(&Problem::MissingField { key: "id".into() })
    );
    assert!(m.problems.contains(&Problem::MissingField {
        key: "title".into()
    }));
}

#[test]
fn broken_yaml_loads_is_flagged_and_refuses_to_render() {
    let raw = "---\nid: m1\ntitle: [unclosed\n---\n\n## Summary\nStill readable.\n";
    let m = Meeting::parse(raw);
    assert!(
        matches!(m.problems[0], Problem::BadFrontmatter { .. }),
        "{:?}",
        m.problems
    );
    // Unreadable YAML is not reported a second time as missing fields.
    assert!(
        !m.problems
            .iter()
            .any(|p| matches!(p, Problem::MissingField { .. }))
    );
    // The body still loads, so the UI can show what is there.
    assert_eq!(m.section("Summary"), Some("Still readable.\n"));
    assert!(matches!(m.render(), Err(Error::Frontmatter { .. })));

    let _dir_guard = temp_dir("broken");
    let dir = _dir_guard.path().to_path_buf();
    let path = dir.join(MEETING_FILE);
    std::fs::write(&path, raw).unwrap();
    let loaded = Meeting::read(&path).unwrap().unwrap();
    match loaded.write(&path) {
        Err(Error::Frontmatter { path: p, .. }) => assert!(p.ends_with(MEETING_FILE)),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
}

#[test]
fn the_body_round_trips_byte_for_byte() {
    let raw = "---
id: m1
title: T
---
Preamble the agent left, with trailing spaces.\x20\x20

## Summary
Line one.
### Not a section
####### Nor this
##Nor this

```md
text
```

## Custom
## Decisions


- two blank lines above
## Action Items
## Open Questions
last line, no newline";
    let m = Meeting::parse(raw);
    assert_eq!(m.problems, vec![]);
    assert_eq!(body_of(&m.render().unwrap()), body_of(raw));
    assert_eq!(m.sections.len(), 5);
    assert!(m.preamble.ends_with("spaces.  \n\n"));
}

#[test]
fn crlf_line_endings_round_trip() {
    let raw = "---\r\nid: m1\r\ntitle: T\r\n---\r\n\r\n## Summary\r\ns\r\n\r\n## Decisions\r\n## Action Items\r\n## Open Questions\r\n";
    let m = Meeting::parse(raw);
    assert_eq!(m.problems, vec![]);
    assert_eq!(m.section("Summary"), Some("s\r\n\r\n"));
    assert_eq!(body_of(&m.render().unwrap()), body_of(raw));
}

#[test]
fn a_new_meeting_parses_back_with_no_problems() {
    let m = Meeting::new("2026-09-01-1430-standup", "Platform: Standup #2");
    let rendered = m.render().unwrap();
    assert_eq!(
        body_of(&rendered),
        "\n## Summary\n\n## Decisions\n\n## Action Items\n\n## Open Questions\n"
    );
    let again = Meeting::parse(&rendered);
    assert_eq!(again.problems, vec![]);
    assert_eq!(again.title().as_deref(), Some("Platform: Standup #2"));
    assert_eq!(again, m);
}

#[test]
fn set_section_replaces_in_place_and_keeps_headings_apart() {
    let mut m = Meeting::new("m1", "T");
    m.set_section("decisions", "- Ship it");
    m.set_section("Open Questions", "- None");
    let again = Meeting::parse(&m.render().unwrap());
    assert_eq!(again.problems, vec![]);
    assert_eq!(again.section("Decisions"), Some("- Ship it\n\n"));
    assert_eq!(again.section("Open Questions"), Some("- None\n"));
    assert_eq!(again.sections[1].heading, "Decisions");
}

#[test]
fn set_section_appends_a_missing_heading_after_a_blank_line() {
    let mut m = Meeting::parse("---\nid: m1\ntitle: T\n---\n## Summary\nno newline at end");
    m.set_section("Decisions", "d");
    assert_eq!(
        body_of(&m.render().unwrap()),
        "## Summary\nno newline at end\n\n## Decisions\nd\n"
    );
}

#[test]
fn a_file_without_frontmatter_is_all_body() {
    let raw = "Notes the agent forgot to fence.\n\n## Summary\nhi\n";
    let m = Meeting::parse(raw);
    assert_eq!(m.problems[0], Problem::NoFrontmatter);
    assert!(
        m.problems
            .contains(&Problem::MissingField { key: "id".into() })
    );
    assert!(m.frontmatter.is_empty());
    assert_eq!(m.preamble, "Notes the agent forgot to fence.\n\n");
    assert_eq!(m.section("Summary"), Some("hi\n"));
    // Nothing is lost by writing a frontmatter block on top.
    let again = Meeting::parse(&m.render().unwrap());
    assert_eq!(again.preamble, m.preamble);
    assert_eq!(again.sections, m.sections);
}

#[test]
fn a_missing_file_reads_as_none() {
    let _dir_guard = temp_dir("missing");
    let dir = _dir_guard.path().to_path_buf();
    assert_eq!(Meeting::read(&dir.join(MEETING_FILE)).unwrap(), None);
}

#[test]
fn non_utf8_loads_as_unreadable_and_is_never_overwritten() {
    let _dir_guard = temp_dir("binary");
    let dir = _dir_guard.path().to_path_buf();
    let path = dir.join(MEETING_FILE);
    let bytes = b"---\nid: m1\n---\n\xff\xfe";
    std::fs::write(&path, bytes).unwrap();
    let m = Meeting::read(&path).unwrap().unwrap();
    assert!(matches!(m.problems[..], [Problem::Unreadable { .. }]));
    assert!(m.sections.is_empty());
    assert!(m.render().is_err());
    assert!(m.write(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn write_then_read_gives_the_same_meeting() {
    let _dir_guard = temp_dir("write");
    let dir = _dir_guard.path().to_path_buf();
    let path = dir.join(MEETING_FILE);
    let m = Meeting::parse(FULL);
    m.write(&path).unwrap();
    assert_eq!(Meeting::read(&path).unwrap(), Some(m));
}
