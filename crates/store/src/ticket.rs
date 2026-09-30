//! `tickets/TICK-NNNN.md` (SPEC §3.3).
//!
//! Tickets are written by the agent and, from TUR-102, by hand in the UI. The
//! same two rules as `meeting.md` apply: unknown keys survive, and a broken
//! ticket loads with a problem attached instead of hiding its neighbours.

use std::path::Path;

use yaml_rust2::Yaml;

use crate::frontmatter::{self, Document, Frontmatter};
use crate::{Error, Problem, refusal};

/// The §3.3 keys in file order, after `id`, `title`, `meeting` and `status`.
/// A new ticket writes each of them as `null`, so whoever edits the file by
/// hand sees every field there is to fill in.
const NULL_KEYS: [&str; 7] = [
    "assignee",
    "estimate",
    "estimated_on",
    "transcript_ref",
    "synced_to",
    "external_id",
    "external_url",
];

/// `status:` values (SPEC §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    InProgress,
    Done,
    Dropped,
}

impl Status {
    /// The literal written to the file: `open`, `in_progress`, `done`,
    /// `dropped`.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::InProgress => "in_progress",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }

    /// The exact literal only: `Open`, ` open` and `in-progress` are all
    /// `None`, so a near-miss is flagged rather than quietly accepted.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open" => Some(Status::Open),
            "in_progress" => Some(Status::InProgress),
            "done" => Some(Status::Done),
            "dropped" => Some(Status::Dropped),
            _ => None,
        }
    }
}

/// A parsed ticket file.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    pub frontmatter: Frontmatter,
    /// Everything after the frontmatter, verbatim.
    pub body: String,
    /// What needs attention. Empty for a well-formed file.
    pub problems: Vec<Problem>,
}

impl Ticket {
    /// A new ticket in the §3.3 key order: `id`, `title`, `meeting`,
    /// `status: open`, and the remaining §3.3 keys present as `null`.
    pub fn new(id: &str, title: &str, meeting: &str) -> Self {
        let mut frontmatter = Frontmatter::new();
        frontmatter.set_str("id", Some(id));
        frontmatter.set_str("title", Some(title));
        frontmatter.set_str("meeting", Some(meeting));
        frontmatter.set_str("status", Some(Status::Open.as_str()));
        for key in NULL_KEYS {
            frontmatter.set_str(key, None);
        }
        Ticket {
            frontmatter,
            body: String::new(),
            problems: Vec::new(),
        }
    }

    /// Parse file contents. Never fails.
    ///
    /// * No / broken frontmatter → same handling as `Meeting::parse`.
    /// * `id` or `title` missing → `Problem::MissingField`.
    /// * `status` present but not a §3.3 value → `Problem::BadField`.
    pub fn parse(raw: &str) -> Self {
        let mut problems = Vec::new();
        let (frontmatter, body, fields_known) = match frontmatter::parse(raw) {
            Ok(doc) => (doc.frontmatter, doc.body, true),
            Err(problem) => {
                // As in `Meeting::parse`: broken YAML hides the fields rather
                // than proving them missing, so do not report them twice.
                let fields_known = problem == Problem::NoFrontmatter;
                problems.push(problem);
                let (_, body) = frontmatter::split(raw);
                (Frontmatter::new(), body.to_owned(), fields_known)
            }
        };

        if fields_known {
            for key in ["id", "title"] {
                if frontmatter.get_str(key).is_none() {
                    problems.push(Problem::MissingField {
                        key: key.to_owned(),
                    });
                }
            }
        }

        // Only `id` and `title` are required, so a missing or `null` status
        // is left alone. Anything else must be one of the four literals.
        match frontmatter.get("status") {
            None | Some(Yaml::Null) => {}
            Some(_) => match frontmatter.get_str("status") {
                Some(s) if Status::parse(&s).is_some() => {}
                written => problems.push(Problem::BadField {
                    key: "status".to_owned(),
                    detail: match written {
                        Some(s) => format!("{s:?} is not one of open, in_progress, done, dropped"),
                        None => "not a single value".to_owned(),
                    },
                }),
            },
        }

        Ticket {
            frontmatter,
            body,
            problems,
        }
    }

    /// Read a ticket file. Non-UTF-8 content loads with `Problem::Unreadable`.
    pub fn read(path: &Path) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        Ok(match String::from_utf8(bytes) {
            Ok(raw) => Self::parse(&raw),
            Err(e) => Ticket {
                frontmatter: Frontmatter::new(),
                body: String::new(),
                problems: vec![Problem::Unreadable {
                    detail: e.to_string(),
                }],
            },
        })
    }

    /// Render back to text. Refuses with [`Error::Frontmatter`] when the file
    /// was loaded with broken frontmatter.
    ///
    /// A ticket that loaded as `Problem::Unreadable` is refused too, with an
    /// [`Error::Io`] of kind `InvalidData`, for the same reason as
    /// `Meeting::render`: none of the bytes on disk are held here.
    pub fn render(&self) -> Result<String, Error> {
        let name = self.id().unwrap_or_else(|| "ticket".to_owned());
        if let Some(refusal) = refusal(&self.problems, &name) {
            return Err(refusal);
        }
        Ok(frontmatter::render(&Document {
            frontmatter: self.frontmatter.clone(),
            body: self.body.clone(),
        }))
    }

    /// Render and write atomically.
    pub fn write(&self, path: &Path) -> Result<(), Error> {
        if let Some(refusal) = refusal(&self.problems, &path.to_string_lossy()) {
            return Err(refusal);
        }
        crate::write_atomic(path, &self.render()?)
    }

    pub fn id(&self) -> Option<String> {
        self.frontmatter.get_str("id")
    }
    pub fn title(&self) -> Option<String> {
        self.frontmatter.get_str("title")
    }
    /// The meeting folder id this ticket came from.
    pub fn meeting(&self) -> Option<String> {
        self.frontmatter.get_str("meeting")
    }
    /// `None` if missing or not a §3.3 value (the latter is also a problem).
    pub fn status(&self) -> Option<Status> {
        self.frontmatter
            .get_str("status")
            .as_deref()
            .and_then(Status::parse)
    }
    pub fn set_status(&mut self, status: Status) {
        self.frontmatter.set_str("status", Some(status.as_str()));
    }
    pub fn assignee(&self) -> Option<String> {
        self.frontmatter.get_str("assignee")
    }
    pub fn estimate(&self) -> Option<String> {
        self.frontmatter.get_str("estimate")
    }
    pub fn transcript_ref(&self) -> Option<String> {
        self.frontmatter.get_str("transcript_ref")
    }
    pub fn synced_to(&self) -> Option<String> {
        self.frontmatter.get_str("synced_to")
    }
}

/// `TICK-0042` → `Some(42)`. Exactly `TICK-` plus four or more digits.
pub fn parse_id(id: &str) -> Option<u32> {
    let digits = id.strip_prefix("TICK-")?;
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Only overflow can fail here; a number past u32 is not an id we issued.
    digits.parse().ok()
}

/// `42` → `TICK-0042`. Numbers past 9999 keep growing (`TICK-10000`).
pub fn format_id(n: u32) -> String {
    format!("TICK-{n:04}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = "---
id: TICK-0001
title: Move sessions to Redis
meeting: 2026-09-01-1430-standup
status: open                  # open | in_progress | done | dropped
assignee: Shantanu
estimate: 2d                  # U4 data collection starts in v1
estimated_on: 2026-09-01
transcript_ref: \"00:14:22\"    # powers Start Work context
synced_to: null               # linear | jira | github
external_id: null
external_url: null
---

Body / acceptance notes.
";

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meet-ai-store-ticket-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bad_status(t: &Ticket) -> bool {
        t.problems
            .iter()
            .any(|p| matches!(p, Problem::BadField { key, .. } if key == "status"))
    }

    #[test]
    fn the_spec_example_parses_cleanly() {
        let t = Ticket::parse(SPEC_EXAMPLE);
        assert_eq!(t.problems, vec![]);
        assert_eq!(t.id().as_deref(), Some("TICK-0001"));
        assert_eq!(t.title().as_deref(), Some("Move sessions to Redis"));
        assert_eq!(t.meeting().as_deref(), Some("2026-09-01-1430-standup"));
        assert_eq!(t.status(), Some(Status::Open));
        assert_eq!(t.assignee().as_deref(), Some("Shantanu"));
        assert_eq!(t.estimate().as_deref(), Some("2d"));
        assert_eq!(t.transcript_ref().as_deref(), Some("00:14:22"));
        assert_eq!(t.synced_to(), None);
        assert_eq!(t.body, "\nBody / acceptance notes.\n");
    }

    #[test]
    fn a_new_ticket_has_the_spec_key_order_and_parses_back_cleanly() {
        let t = Ticket::new("TICK-0042", "Fix: the thing", "2026-09-01-1430-standup");
        assert_eq!(
            t.frontmatter.keys(),
            vec![
                "id",
                "title",
                "meeting",
                "status",
                "assignee",
                "estimate",
                "estimated_on",
                "transcript_ref",
                "synced_to",
                "external_id",
                "external_url",
            ]
        );
        assert_eq!(t.status(), Some(Status::Open));
        assert_eq!(t.assignee(), None);
        assert_eq!(t.body, "");
        let again = Ticket::parse(&t.render().unwrap());
        assert_eq!(again.problems, vec![]);
        assert_eq!(again, t);
    }

    #[test]
    fn unknown_frontmatter_keys_survive_a_round_trip() {
        let raw = SPEC_EXAMPLE.replace(
            "external_url: null\n",
            "external_url: null\npriority: high\nlabels: [infra, redis]\n",
        );
        let t = Ticket::parse(&raw);
        assert_eq!(t.problems, vec![]);
        let again = Ticket::parse(&t.render().unwrap());
        assert_eq!(again.frontmatter, t.frontmatter);
        assert_eq!(
            again.frontmatter.get_str("priority").as_deref(),
            Some("high")
        );
        assert_eq!(
            again.frontmatter.get_str_list("labels"),
            Some(vec!["infra".to_owned(), "redis".to_owned()])
        );
        assert_eq!(
            again.frontmatter.keys().last().map(String::as_str),
            Some("labels")
        );
    }

    #[test]
    fn the_body_round_trips_byte_for_byte() {
        let raw = "---\nid: TICK-0002\ntitle: T\n---\n## Acceptance\n\n- [ ] one  \n\n```\n---\n```\nno newline";
        let t = Ticket::parse(raw);
        assert_eq!(t.problems, vec![]);
        assert_eq!(
            t.body,
            "## Acceptance\n\n- [ ] one  \n\n```\n---\n```\nno newline"
        );
        assert_eq!(Ticket::parse(&t.render().unwrap()).body, t.body);
    }

    #[test]
    fn a_bad_status_is_flagged_and_the_rest_still_loads() {
        let mut t = Ticket::parse(&SPEC_EXAMPLE.replace("status: open ", "status: maybe"));
        assert!(bad_status(&t), "{:?}", t.problems);
        assert_eq!(t.problems.len(), 1);
        assert_eq!(t.status(), None);
        assert_eq!(t.title().as_deref(), Some("Move sessions to Redis"));
        // A flagged ticket is still writable, and fixing the status clears it.
        t.set_status(Status::Done);
        let again = Ticket::parse(&t.render().unwrap());
        assert_eq!(again.problems, vec![]);
        assert_eq!(again.status(), Some(Status::Done));
    }

    #[test]
    fn a_status_that_is_a_list_is_flagged() {
        let t = Ticket::parse("---\nid: TICK-0001\ntitle: T\nstatus: [open, done]\n---\n");
        assert!(bad_status(&t), "{:?}", t.problems);
    }

    #[test]
    fn a_missing_or_null_status_is_not_a_problem() {
        for raw in [
            "---\nid: TICK-0001\ntitle: T\n---\n",
            "---\nid: TICK-0001\ntitle: T\nstatus: null\n---\n",
            "---\nid: TICK-0001\ntitle: T\nstatus: ~\n---\n",
        ] {
            let t = Ticket::parse(raw);
            assert_eq!(t.problems, vec![], "{raw:?}");
            assert_eq!(t.status(), None);
        }
    }

    #[test]
    fn missing_id_and_title_are_flagged() {
        let t = Ticket::parse("---\nstatus: open\n---\nbody\n");
        assert_eq!(
            t.problems,
            vec![
                Problem::MissingField { key: "id".into() },
                Problem::MissingField {
                    key: "title".into()
                },
            ]
        );
        assert_eq!(t.body, "body\n");
    }

    #[test]
    fn a_ticket_without_frontmatter_is_all_body() {
        let t = Ticket::parse("Just some text.\n");
        assert_eq!(t.problems[0], Problem::NoFrontmatter);
        assert_eq!(t.body, "Just some text.\n");
        assert_eq!(Ticket::parse(&t.render().unwrap()).body, t.body);
    }

    #[test]
    fn broken_yaml_loads_is_flagged_and_refuses_to_render() {
        let raw = "---\nid: TICK-0003\ntitle: [unclosed\n---\nKeep me.\n";
        let t = Ticket::parse(raw);
        assert!(
            matches!(t.problems[..], [Problem::BadFrontmatter { .. }]),
            "{:?}",
            t.problems
        );
        assert_eq!(t.body, "Keep me.\n");
        assert!(matches!(t.render(), Err(Error::Frontmatter { .. })));

        let dir = temp_dir("broken");
        let path = dir.join("TICK-0003.md");
        std::fs::write(&path, raw).unwrap();
        let loaded = Ticket::read(&path).unwrap();
        match loaded.write(&path) {
            Err(Error::Frontmatter { path: p, .. }) => assert!(p.ends_with("TICK-0003.md")),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_utf8_loads_as_unreadable_and_is_never_overwritten() {
        let dir = temp_dir("binary");
        let path = dir.join("TICK-0004.md");
        let bytes = b"---\nid: TICK-0004\n---\n\xc3\x28";
        std::fs::write(&path, bytes).unwrap();
        let t = Ticket::read(&path).unwrap();
        assert!(matches!(t.problems[..], [Problem::Unreadable { .. }]));
        assert!(t.render().is_err());
        assert!(t.write(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_ticket_file_is_an_error() {
        let dir = temp_dir("missing");
        assert!(matches!(
            Ticket::read(&dir.join("TICK-0009.md")),
            Err(Error::Io(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_then_read_gives_the_same_ticket() {
        let dir = temp_dir("write");
        let path = dir.join("TICK-0001.md");
        let t = Ticket::parse(SPEC_EXAMPLE);
        t.write(&path).unwrap();
        assert_eq!(Ticket::read(&path).unwrap(), t);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_parse_accepts_only_the_four_literals() {
        for s in [
            Status::Open,
            Status::InProgress,
            Status::Done,
            Status::Dropped,
        ] {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        for near_miss in [
            "Open",
            " open",
            "open ",
            "in-progress",
            "inProgress",
            "closed",
            "",
        ] {
            assert_eq!(Status::parse(near_miss), None, "{near_miss:?}");
        }
    }

    #[test]
    fn parse_id_takes_tick_and_four_or_more_digits() {
        assert_eq!(parse_id("TICK-0001"), Some(1));
        assert_eq!(parse_id("TICK-0042"), Some(42));
        assert_eq!(parse_id("TICK-9999"), Some(9999));
        assert_eq!(parse_id("TICK-12345"), Some(12345));
        for bad in [
            "TICK-12",
            "TICK-001",
            "tick-0001",
            "TICK-00a1",
            "TICK-",
            "TICK-+001",
            "TICK- 0001",
            "TICK-0001.md",
            "XTICK-0001",
            "TICK-99999999999",
        ] {
            assert_eq!(parse_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn format_id_pads_to_four_digits_and_then_grows() {
        assert_eq!(format_id(1), "TICK-0001");
        assert_eq!(format_id(42), "TICK-0042");
        assert_eq!(format_id(9999), "TICK-9999");
        assert_eq!(format_id(10000), "TICK-10000");
        assert_eq!(format_id(12345), "TICK-12345");
        for n in [0, 7, 9999, 10000, u32::MAX] {
            assert_eq!(parse_id(&format_id(n)), Some(n));
        }
    }
}
