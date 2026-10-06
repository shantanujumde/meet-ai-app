//! The notes the agent sends back after a meeting, and the check they must pass.
//!
//! SPEC A11, "Data contract changes": the agent no longer writes `meeting.md`
//! itself. It answers with one JSON object (a title, a summary, the decisions,
//! the open questions and the tasks) and the app writes the files from it. This module is
//! the one definition of that object:
//!
//! * [`NOTES_SCHEMA`] — the JSON schema as text. `crates/agent` hands it to the
//!   CLI: Claude Code takes it with `--json-schema`, Codex with
//!   `--output-schema`.
//! * [`notes_schema`] — the same schema, parsed once.
//! * [`Notes`] and [`Task`] — the Rust shape of an answer that passed.
//! * [`validate`], [`Notes::from_value`], [`Notes::from_json`] — the check. An
//!   answer that fails it is never written anywhere (SPEC A11, "Guarding
//!   against the meeting itself"); the caller gets [`Error::InvalidNotes`] with
//!   one line per problem.
//!
//! The schema is in the strict form both CLIs accept. Codex runs it through
//! OpenAI's structured-output strict mode, which insists that every object
//! lists all of its fields in `required` and forbids extra fields. So a field
//! that can be empty is `null`, never missing. The schema sticks to plain
//! keywords (`type`, `properties`, `required`, `additionalProperties`, `items`,
//! `pattern`, `description`) because strict mode rejects others, such as
//! `format` or `minItems`. A unit test walks the schema to keep it that way.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;

/// The notes JSON schema, as pretty-printed text.
///
/// `crates/agent` writes this to a file or passes it on the command line
/// as-is. The `description` on each field is part of the prompt: the model
/// reads it, so it says in plain words what goes there.
///
/// `transcript_ref` must be bare `HH:MM:SS`. In a test Codex copied the
/// transcript's `[00:00:05]` brackets and all; the pattern rejects that, so the
/// app can always find the line again.
pub const NOTES_SCHEMA: &str = r#"{
  "type": "object",
  "description": "Notes from one meeting, taken from its transcript.",
  "properties": {
    "title": {
      "type": "string",
      "description": "A short name for the meeting, 3 to 6 words, like a calendar event title."
    },
    "summary": {
      "type": "string",
      "description": "A short summary of what the meeting was about and what came out of it."
    },
    "decisions": {
      "type": "array",
      "description": "Decisions the people in the meeting agreed on, one per item. Empty if there were none.",
      "items": {
        "type": "string"
      }
    },
    "open_questions": {
      "type": "array",
      "description": "Questions raised in the meeting that nobody answered, one per item. Empty if there were none.",
      "items": {
        "type": "string"
      }
    },
    "tasks": {
      "type": "array",
      "description": "Work someone said would be done. Empty if there was none.",
      "items": {
        "type": "object",
        "properties": {
          "title": {
            "type": "string",
            "description": "A short title for the task, like a ticket title."
          },
          "details": {
            "type": "string",
            "description": "What to do and why, in 1 to 3 sentences, from the meeting. Never empty: it is the ticket's description."
          },
          "owner": {
            "type": ["string", "null"],
            "description": "Person who committed to the task, or null if nobody did."
          },
          "due": {
            "type": ["string", "null"],
            "description": "Date or day as said in the meeting, or null if none was said."
          },
          "transcript_ref": {
            "type": "string",
            "description": "Start time of the line the task comes from, HH:MM:SS, copied from the transcript without the brackets.",
            "pattern": "^\\d{2}:\\d{2}:\\d{2}$"
          }
        },
        "required": ["title", "details", "owner", "due", "transcript_ref"],
        "additionalProperties": false
      }
    }
  },
  "required": ["title", "summary", "decisions", "open_questions", "tasks"],
  "additionalProperties": false
}"#;

/// The longest a single problem line may get, in characters.
///
/// A type error on a big value would otherwise print the whole value, which is
/// the agent's answer and so can be a large slice of the meeting.
const MAX_PROBLEM_CHARS: usize = 300;

/// [`NOTES_SCHEMA`], parsed once.
pub fn notes_schema() -> &'static Value {
    static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
        // quality: allow-unwrap NOTES_SCHEMA is a literal, parsed by a unit test
        serde_json::from_str(NOTES_SCHEMA).expect("NOTES_SCHEMA is valid JSON")
    });
    &SCHEMA
}

/// The compiled check for [`NOTES_SCHEMA`], built once on first use.
fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
        // quality: allow-unwrap NOTES_SCHEMA is a literal, parsed by a unit test
        jsonschema::draft202012::new(notes_schema()).expect("NOTES_SCHEMA is a valid schema")
    });
    &VALIDATOR
}

/// The notes from one meeting, as the agent sent them and the check passed.
///
/// Field for field the same as [`NOTES_SCHEMA`]. Unknown fields are refused, so
/// this type and the schema cannot quietly drift apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    /// A short name for the meeting, like a calendar event title (TUR-103).
    /// It replaces a calendar title, never one the user gave
    /// (`store::meeting_title`).
    pub title: String,
    /// A short summary of the meeting.
    pub summary: String,
    /// What the people in the meeting agreed on, one per item.
    pub decisions: Vec<String>,
    /// Questions raised and left unanswered, one per item.
    pub open_questions: Vec<String>,
    /// Work someone said would be done.
    pub tasks: Vec<Task>,
}

/// One piece of work from the meeting. The app turns each into a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// A short title, like a ticket title.
    pub title: String,
    /// What has to be done, with context from the meeting.
    pub details: String,
    /// Who committed to it, if anyone did.
    pub owner: Option<String>,
    /// When it is due, in the words used in the meeting ("Friday"), if said.
    pub due: Option<String>,
    /// Start time of the transcript line the task comes from, `HH:MM:SS`.
    pub transcript_ref: String,
}

impl Notes {
    /// Check `value` against [`NOTES_SCHEMA`] and turn it into [`Notes`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidNotes`] if the value fails the schema, or (which the
    /// schema should already rule out) does not fit the Rust types.
    pub fn from_value(value: Value) -> Result<Notes, Error> {
        validate(&value)?;
        serde_json::from_value(value).map_err(|e| Error::InvalidNotes {
            problems: vec![clip(e.to_string())],
        })
    }

    /// Parse the agent's answer as JSON, check it, and turn it into [`Notes`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidNotes`] if `text` is not JSON at all (the problem is the
    /// parser's message) or if the JSON fails the check in
    /// [`Notes::from_value`].
    pub fn from_json(text: &str) -> Result<Notes, Error> {
        let value: Value = serde_json::from_str(text).map_err(|e| Error::InvalidNotes {
            problems: vec![clip(format!("not valid JSON: {e}"))],
        })?;
        Notes::from_value(value)
    }
}

/// Check that `value` matches [`NOTES_SCHEMA`].
///
/// # Errors
///
/// [`Error::InvalidNotes`] with one line per problem. Each line starts with the
/// JSON path of the bad part, for example
/// `/tasks/0/transcript_ref: "[00:00:05]" does not match "^\d{2}:\d{2}:\d{2}$"`.
/// A problem with the whole object starts with `(top level)`.
pub fn validate(value: &Value) -> Result<(), Error> {
    let problems: Vec<String> = validator()
        .iter_errors(value)
        .map(|error| {
            let path = error.instance_path().as_str();
            let path = if path.is_empty() { "(top level)" } else { path };
            clip(format!("{path}: {error}"))
        })
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidNotes { problems })
    }
}

/// Cut `problem` down to [`MAX_PROBLEM_CHARS`] characters, marking the cut.
fn clip(problem: String) -> String {
    match problem.char_indices().nth(MAX_PROBLEM_CHARS) {
        Some((end, _)) => format!("{}…", &problem[..end]),
        None => problem,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The A11 example shape, filled in the way a real meeting would.
    fn example() -> Value {
        json!({
            "title": "Search release planning",
            "summary": "Planned the search release. The box ships first; filters wait for feedback.",
            "decisions": [
                "Ship search without filters first.",
                "Keep the old results page until the new one is stable."
            ],
            "open_questions": ["Do we need search inside archived meetings?"],
            "tasks": [
                {
                    "title": "Ship the search box",
                    "details": "Put the search box on the meetings list, titles only for now.",
                    "owner": "Priya",
                    "due": "Friday",
                    "transcript_ref": "00:00:05"
                },
                {
                    "title": "Find out who uses archived meetings",
                    "details": "Someone should check how often archived meetings are opened.",
                    "owner": null,
                    "due": null,
                    "transcript_ref": "00:12:40"
                }
            ]
        })
    }

    fn problems(value: &Value) -> Vec<String> {
        match validate(value) {
            Err(Error::InvalidNotes { problems }) => problems,
            other => panic!("expected InvalidNotes, got {other:?}"),
        }
    }

    /// Assert that `value` fails, with a problem that starts with `path`.
    fn assert_rejected_at(value: &Value, path: &str) {
        let problems = problems(value);
        assert!(
            problems.iter().any(|p| p.starts_with(&format!("{path}: "))),
            "no problem at {path}: {problems:?}"
        );
        assert!(Notes::from_value(value.clone()).is_err());
    }

    /// Every object node in `node` must forbid extra fields and require all of
    /// its properties, or Codex's strict mode refuses the schema.
    fn assert_strict(node: &Value, at: &str) {
        match node {
            Value::Object(map) => {
                if map.get("type") == Some(&json!("object")) {
                    assert_eq!(
                        map.get("additionalProperties"),
                        Some(&json!(false)),
                        "{at} must set additionalProperties to false"
                    );
                    let mut keys: Vec<&str> = map["properties"]
                        .as_object()
                        .unwrap_or_else(|| panic!("{at} has no properties"))
                        .keys()
                        .map(String::as_str)
                        .collect();
                    let mut required: Vec<&str> = map["required"]
                        .as_array()
                        .unwrap_or_else(|| panic!("{at} has no required list"))
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect();
                    keys.sort_unstable();
                    required.sort_unstable();
                    assert_eq!(keys, required, "{at} must require every property");
                }
                for keyword in ["format", "minItems", "maxItems", "oneOf", "$ref"] {
                    assert!(!map.contains_key(keyword), "{at} uses `{keyword}`");
                }
                for (key, child) in map {
                    assert_strict(child, &format!("{at}/{key}"));
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    assert_strict(child, &format!("{at}/{i}"));
                }
            }
            _ => {}
        }
    }

    #[test]
    fn notes_schema_parses_and_is_strict() {
        let parsed: Value = serde_json::from_str(NOTES_SCHEMA).unwrap();
        assert_eq!(&parsed, notes_schema());
        assert_eq!(notes_schema()["type"], "object");
        assert_strict(notes_schema(), "");
        // Compiling must not panic either.
        let _ = validator();
    }

    #[test]
    fn notes_accepts_the_a11_example_and_round_trips() {
        let value = example();
        validate(&value).unwrap();
        let notes = Notes::from_value(value.clone()).unwrap();
        assert_eq!(notes.title, "Search release planning");
        assert_eq!(notes.decisions.len(), 2);
        assert_eq!(
            notes.open_questions,
            ["Do we need search inside archived meetings?"]
        );
        assert_eq!(
            notes.tasks[0],
            Task {
                title: "Ship the search box".into(),
                details: "Put the search box on the meetings list, titles only for now.".into(),
                owner: Some("Priya".into()),
                due: Some("Friday".into()),
                transcript_ref: "00:00:05".into(),
            }
        );
        assert_eq!(notes.tasks[1].owner, None);
        assert_eq!(notes.tasks[1].due, None);
        assert_eq!(notes.tasks[1].transcript_ref, "00:12:40");
        // Writing it back out gives the same JSON, nulls included.
        assert_eq!(serde_json::to_value(&notes).unwrap(), value);
        assert_eq!(Notes::from_json(&value.to_string()).unwrap(), notes);
    }

    #[test]
    fn notes_accepts_empty_lists() {
        let value = json!({
            "title": "Quick check-in",
            "summary": "A quick check-in; nothing was decided.",
            "decisions": [],
            "open_questions": [],
            "tasks": []
        });
        let notes = Notes::from_value(value).unwrap();
        assert!(notes.decisions.is_empty() && notes.open_questions.is_empty());
        assert!(notes.tasks.is_empty());
    }

    #[test]
    fn notes_rejects_a_transcript_ref_not_in_hh_mm_ss() {
        // `\d` is `[0-9]` here (ECMA rules), so other scripts' digits fail too,
        // and `$` is the true end, so a trailing newline fails.
        for bad in ["[00:00:05]", "0:00:05", "00:00:5", "٠٠:٠٠:٠٥", "00:00:05\n"] {
            let mut value = example();
            value["tasks"][0]["transcript_ref"] = json!(bad);
            assert_rejected_at(&value, "/tasks/0/transcript_ref");
        }
        let mut value = example();
        value["tasks"][0]["transcript_ref"] = json!("[00:00:05]");
        assert_eq!(
            problems(&value),
            [r#"/tasks/0/transcript_ref: "[00:00:05]" does not match "^\d{2}:\d{2}:\d{2}$""#]
        );
    }

    #[test]
    fn notes_rejects_an_extra_top_level_field() {
        let mut value = example();
        value["mood"] = json!("upbeat");
        assert_rejected_at(&value, "(top level)");
        assert!(problems(&value)[0].contains("mood"));
    }

    #[test]
    fn notes_rejects_an_extra_field_inside_a_task() {
        let mut value = example();
        value["tasks"][1]["priority"] = json!("high");
        assert_rejected_at(&value, "/tasks/1");
        assert!(problems(&value)[0].contains("priority"));
    }

    #[test]
    fn notes_rejects_a_missing_field_instead_of_null() {
        let mut value = example();
        value["tasks"][1].as_object_mut().unwrap().remove("due");
        assert_rejected_at(&value, "/tasks/1");
        assert!(problems(&value)[0].contains("due"));

        let mut value = example();
        value.as_object_mut().unwrap().remove("open_questions");
        assert_rejected_at(&value, "(top level)");

        let mut value = example();
        value.as_object_mut().unwrap().remove("title");
        assert_rejected_at(&value, "(top level)");
        assert!(problems(&value)[0].contains("title"));
    }

    #[test]
    fn notes_rejects_a_wrong_type() {
        let mut value = example();
        value["tasks"][0]["owner"] = json!(5);
        assert_rejected_at(&value, "/tasks/0/owner");

        let mut value = example();
        value["decisions"] = json!("Ship it");
        assert_rejected_at(&value, "/decisions");

        let mut value = example();
        value["title"] = json!(null);
        assert_rejected_at(&value, "/title");
    }

    #[test]
    fn notes_lists_every_problem() {
        let mut value = example();
        value["tasks"][0]["owner"] = json!(5);
        value["tasks"][1]["transcript_ref"] = json!("[00:12:40]");
        assert_eq!(problems(&value).len(), 2);
    }

    #[test]
    fn notes_from_json_rejects_text_that_is_not_json() {
        match Notes::from_json("not json") {
            Err(Error::InvalidNotes { problems }) => {
                assert_eq!(problems.len(), 1);
                assert!(problems[0].starts_with("not valid JSON: "), "{problems:?}");
            }
            other => panic!("expected InvalidNotes, got {other:?}"),
        }
    }

    #[test]
    fn notes_problem_lines_are_clipped() {
        let long = "x".repeat(MAX_PROBLEM_CHARS * 2);
        let clipped = clip(long);
        assert_eq!(clipped.chars().count(), MAX_PROBLEM_CHARS + 1);
        assert!(clipped.ends_with('…'));
        assert_eq!(clip("short".into()), "short");
    }
}
