//! `meeting.md` — frontmatter plus the four fixed sections (SPEC §3.2).
//!
//! Sections are located by heading, never by position: on the copy-prompt
//! fallback the agent writes this file, and SPEC §3.2 only promises it will not
//! *rename* the headings. It may
//! reorder them, leave one out, or add a heading of its own. All of that must
//! load, and anything this code does not model must survive a write.
//!
//! A section starts at a line beginning with exactly `## ` (two hashes and a
//! space). `### ` and deeper headings are ordinary body text, so an agent can
//! structure a section without splitting it.
//!
//! The body round-trips byte for byte, with one normalisation: a heading line
//! is written back as `## <trimmed heading>` plus a line ending. Stray spaces
//! around the heading text are dropped, because [`Section::heading`] is the
//! trimmed text and there is nowhere public to keep the original. The line
//! ending follows the file: `\r\n` if the rest of the body uses it, `\n`
//! otherwise, so a file saved on Windows is not turned into mixed endings.

use std::path::Path;

use crate::frontmatter::{self, Document, Frontmatter};
use crate::{Error, MEETING_FILE, Problem, refusal};

/// The fixed `##` headings, in the order a new file is written with.
pub const SECTIONS: [&str; 4] = ["Summary", "Decisions", "Action Items", "Open Questions"];

/// One `## Heading` and the text under it, up to the next `## ` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The heading text without `## `, trimmed.
    pub heading: String,
    /// Everything between this heading line and the next, verbatim.
    pub body: String,
}

/// A parsed `meeting.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct Meeting {
    pub frontmatter: Frontmatter,
    /// Text after the frontmatter and before the first `## ` heading.
    pub preamble: String,
    /// Every `## ` section in file order, including ones not in [`SECTIONS`].
    pub sections: Vec<Section>,
    /// What needs attention. Empty for a well-formed file.
    pub problems: Vec<Problem>,
}

impl Meeting {
    /// A fresh file for a new meeting: `id` and `title` set, all four
    /// sections present and empty.
    pub fn new(id: &str, title: &str) -> Self {
        let mut frontmatter = Frontmatter::new();
        frontmatter.set_str("id", Some(id));
        frontmatter.set_str("title", Some(title));
        let mut meeting = Meeting {
            frontmatter,
            // A blank line between the closing `---` and the first heading.
            preamble: "\n".to_owned(),
            sections: Vec::new(),
            problems: Vec::new(),
        };
        for heading in SECTIONS {
            meeting.set_section(heading, "");
        }
        meeting
    }

    /// Parse file contents. Never fails — a broken file comes back with
    /// `problems` filled in.
    ///
    /// * No frontmatter → `Problem::NoFrontmatter`, empty frontmatter, the
    ///   whole file parsed as body.
    /// * Broken frontmatter → `Problem::BadFrontmatter`, and the file is
    ///   remembered as unwritable (see [`Meeting::render`]).
    /// * Each of [`SECTIONS`] not found → one `Problem::MissingSection`.
    /// * `id` or `title` missing → `Problem::MissingField`.
    pub fn parse(raw: &str) -> Self {
        let mut problems = Vec::new();
        let (frontmatter, body, fields_known) = match frontmatter::parse(raw) {
            Ok(doc) => (doc.frontmatter, doc.body, true),
            Err(problem) => {
                // With broken YAML the keys may well be there, just unreadable,
                // so "id is missing" would be a false second alarm. With no
                // block at all they really are missing.
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

        let (preamble, sections) = split_sections(&body);
        let mut meeting = Meeting {
            frontmatter,
            preamble,
            sections,
            problems,
        };
        for heading in SECTIONS {
            if meeting.section(heading).is_none() {
                meeting.problems.push(Problem::MissingSection {
                    heading: heading.to_owned(),
                });
            }
        }
        meeting
    }

    /// Read `meeting.md` at `path`. `Ok(None)` if it does not exist — a
    /// meeting nobody has wrapped up yet is the normal case. Non-UTF-8 content
    /// loads as an empty meeting with `Problem::Unreadable`.
    pub fn read(path: &Path) -> Result<Option<Self>, Error> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(match String::from_utf8(bytes) {
            Ok(raw) => Self::parse(&raw),
            Err(e) => Meeting {
                frontmatter: Frontmatter::new(),
                preamble: String::new(),
                sections: Vec::new(),
                problems: vec![Problem::Unreadable {
                    detail: e.to_string(),
                }],
            },
        }))
    }

    /// Render back to text. Refuses with [`Error::Frontmatter`] when the file
    /// was loaded with broken frontmatter, rather than replacing it with an
    /// empty block.
    ///
    /// A file that loaded as `Problem::Unreadable` is refused too, with an
    /// [`Error::Io`] of kind `InvalidData`: this struct holds none of the bytes
    /// that were on disk, so writing it would erase them.
    pub fn render(&self) -> Result<String, Error> {
        if let Some(refusal) = refusal(&self.problems, MEETING_FILE) {
            return Err(refusal);
        }
        Ok(frontmatter::render(&Document {
            frontmatter: self.frontmatter.clone(),
            body: self.render_body(),
        }))
    }

    /// Render and write atomically (see [`crate::write_atomic`]).
    pub fn write(&self, path: &Path) -> Result<(), Error> {
        if let Some(refusal) = refusal(&self.problems, &path.to_string_lossy()) {
            return Err(refusal);
        }
        crate::write_atomic(path, &self.render()?)
    }

    /// The body under `## heading`, matched case-insensitively on trimmed
    /// text.
    pub fn section(&self, heading: &str) -> Option<&str> {
        self.position(heading)
            .map(|i| self.sections[i].body.as_str())
    }

    /// Replace the body under `## heading`, or append a new section at the
    /// end if it is missing.
    ///
    /// `body` is stored as given apart from its ending: it is made to end
    /// with a line break, plus a blank line when another section follows, so
    /// the next heading always starts a paragraph of its own. When a section
    /// is appended, the section before it gets the same blank line.
    pub fn set_section(&mut self, heading: &str, body: &str) {
        let eol = self.eol();
        let count = self.sections.len();
        if let Some(i) = self.position(heading) {
            let followed = i + 1 < count;
            self.sections[i].body = finish_body(body, followed, eol);
            return;
        }
        match self.sections.last_mut() {
            Some(previous) => end_with_blank_line(&mut previous.body, eol),
            None => end_with_blank_line(&mut self.preamble, eol),
        }
        self.sections.push(Section {
            heading: heading.trim().to_owned(),
            body: finish_body(body, false, eol),
        });
    }

    pub fn id(&self) -> Option<String> {
        self.frontmatter.get_str("id")
    }
    pub fn title(&self) -> Option<String> {
        self.frontmatter.get_str("title")
    }
    /// ISO-8601 as written, e.g. `2026-09-01T14:30:00+05:30`.
    pub fn date(&self) -> Option<String> {
        self.frontmatter.get_str("date")
    }
    pub fn duration_sec(&self) -> Option<i64> {
        self.frontmatter.get_i64("duration_sec")
    }
    pub fn attendees(&self) -> Vec<String> {
        self.frontmatter
            .get_str_list("attendees")
            .unwrap_or_default()
    }
    /// The notes have been written (`analyzed_by` is set).
    pub fn is_analyzed(&self) -> bool {
        self.frontmatter.get_str("analyzed_by").is_some()
    }

    fn position(&self, heading: &str) -> Option<usize> {
        let wanted = heading.trim().to_lowercase();
        self.sections
            .iter()
            .position(|s| s.heading.trim().to_lowercase() == wanted)
    }

    /// The line ending this file uses, judged from the text around the
    /// headings.
    fn eol(&self) -> &'static str {
        let crlf =
            self.preamble.contains("\r\n") || self.sections.iter().any(|s| s.body.contains("\r\n"));
        if crlf { "\r\n" } else { "\n" }
    }

    /// Everything after the frontmatter: the preamble, then each section.
    fn render_body(&self) -> String {
        let eol = self.eol();
        let mut out = self.preamble.clone();
        for section in &self.sections {
            // A parsed body already ends in a line break; one built in code
            // might not, and without this the heading would be glued onto its
            // last line and silently stop being a heading.
            if !out.is_empty() && !out.ends_with('\n') {
                out.push_str(eol);
            }
            out.push_str("## ");
            out.push_str(&section.heading);
            out.push_str(eol);
            out.push_str(&section.body);
        }
        out
    }
}

/// Split a body at every line that starts with `## `.
fn split_sections(body: &str) -> (String, Vec<Section>) {
    let mut preamble = String::new();
    let mut sections: Vec<Section> = Vec::new();
    for line in body.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("## ") {
            sections.push(Section {
                heading: rest.trim().to_owned(),
                body: String::new(),
            });
        } else if let Some(current) = sections.last_mut() {
            current.body.push_str(line);
        } else {
            preamble.push_str(line);
        }
    }
    (preamble, sections)
}

/// Make `text` — which always follows a line break (a heading line or the
/// closing `---`) — end with a blank line.
fn end_with_blank_line(text: &mut String, eol: &str) {
    if !text.ends_with('\n') {
        if !text.is_empty() {
            text.push_str(eol);
        }
        text.push_str(eol);
        return;
    }
    let last_line_start = text[..text.len() - 1].rfind('\n').map_or(0, |i| i + 1);
    if !text[last_line_start..].trim().is_empty() {
        text.push_str(eol);
    }
}

fn finish_body(body: &str, followed: bool, eol: &str) -> String {
    let mut body = body.to_owned();
    if followed {
        end_with_blank_line(&mut body, eol);
    } else if !body.is_empty() && !body.ends_with('\n') {
        body.push_str(eol);
    }
    body
}

#[cfg(test)]
mod tests;
