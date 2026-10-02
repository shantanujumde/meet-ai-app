//! Checking what the CLI printed before any of it reaches disk (SPEC A11,
//! "Guarding against the meeting itself").

use crate::AgentError;

/// Most schema errors reported. A reply that is wrong everywhere would
/// otherwise produce a message nobody reads to the end.
const MAX_SCHEMA_ERRORS: usize = 8;

/// A job's output schema, compiled once per run.
pub struct OutputCheck {
    validator: jsonschema::Validator,
}

impl OutputCheck {
    /// Compiles `schema`. A broken schema is our bug, not the agent's, so it
    /// fails before the CLI is started.
    pub fn new(schema: &serde_json::Value) -> Result<Self, AgentError> {
        jsonschema::validator_for(schema)
            .map(|validator| Self { validator })
            .map_err(|e| AgentError::CouldNotStart {
                reason: format!("the output schema is invalid: {e}"),
            })
    }

    /// Passes `value` through if it matches the schema.
    ///
    /// Error messages are masked: they name the place in the reply
    /// (`/tasks/0/owner`) and the rule it broke, never the text that was there,
    /// since that text comes from the transcript.
    pub fn check(&self, value: serde_json::Value) -> Result<serde_json::Value, AgentError> {
        let errors: Vec<String> = self
            .validator
            .iter_errors(&value)
            .take(MAX_SCHEMA_ERRORS)
            .map(|e| {
                let at = e.instance_path().to_string();
                let at = if at.is_empty() { "/".to_owned() } else { at };
                format!("at {at}: {}", e.masked())
            })
            .collect();
        if errors.is_empty() {
            Ok(value)
        } else {
            Err(AgentError::SchemaMismatch { errors })
        }
    }
}

/// Parses the CLI's reply as one JSON value. Surrounding whitespace is fine;
/// anything else is not.
pub fn parse_json(text: &str) -> Result<serde_json::Value, AgentError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AgentError::InvalidJson {
            reason: "the reply was empty".to_owned(),
        });
    }
    serde_json::from_str(text).map_err(|e| AgentError::InvalidJson {
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["summary", "tasks"],
            "properties": {
                "summary": { "type": "string" },
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["title", "transcript_ref"],
                        "properties": {
                            "title": { "type": "string" },
                            "transcript_ref": { "type": "string", "pattern": "^\\d{2}:\\d{2}:\\d{2}$" }
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn a_matching_reply_passes_unchanged() {
        let check = OutputCheck::new(&schema()).unwrap();
        let reply =
            json!({ "summary": "s", "tasks": [{ "title": "t", "transcript_ref": "00:01:02" }] });
        assert_eq!(check.check(reply.clone()).unwrap(), reply);
    }

    #[test]
    fn a_wrong_reply_says_where_without_quoting_it() {
        let check = OutputCheck::new(&schema()).unwrap();
        let reply = json!({ "summary": "s", "tasks": [{ "title": "t", "transcript_ref": "PRIVATE WORDS" }] });
        let Err(AgentError::SchemaMismatch { errors }) = check.check(reply) else {
            panic!("expected a mismatch")
        };
        let all = errors.join("\n");
        assert!(all.contains("/tasks/0/transcript_ref"), "{all}");
        assert!(!all.contains("PRIVATE"), "{all}");
    }

    #[test]
    fn missing_and_extra_fields_are_both_reported() {
        let check = OutputCheck::new(&schema()).unwrap();
        let Err(AgentError::SchemaMismatch { errors }) = check.check(json!({ "extra": 1 })) else {
            panic!("expected a mismatch")
        };
        assert!(errors.len() >= 2, "{errors:?}");
    }

    #[test]
    fn a_broken_schema_fails_before_the_run() {
        let err = OutputCheck::new(&json!({ "type": 12 })).err().unwrap();
        assert!(matches!(err, AgentError::CouldNotStart { .. }), "{err:?}");
    }

    #[test]
    fn replies_that_are_not_json_are_rejected() {
        assert!(matches!(
            parse_json("  \n"),
            Err(AgentError::InvalidJson { .. })
        ));
        assert!(matches!(
            parse_json("Sure! Here are your notes"),
            Err(AgentError::InvalidJson { .. })
        ));
        assert!(matches!(
            parse_json("{\"a\":1} trailing"),
            Err(AgentError::InvalidJson { .. })
        ));
        assert_eq!(parse_json(" {\"a\":1}\n").unwrap(), json!({ "a": 1 }));
    }
}
