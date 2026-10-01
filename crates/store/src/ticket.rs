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
mod tests;
