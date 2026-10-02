//! Checking what the CLI printed before any of it reaches disk (SPEC A11,
//! "Guarding against the meeting itself").

use std::collections::HashSet;

use jsonschema::error::ValidationErrorKind;

use crate::AgentError;

/// Most schema errors reported. A reply that is wrong everywhere would
/// otherwise produce a message nobody reads to the end.
const MAX_SCHEMA_ERRORS: usize = 8;

/// Stands in for a path segment the schema did not name.
const HIDDEN_SEGMENT: &str = "*";

/// A job's output schema, compiled once per run.
pub struct OutputCheck {
    validator: jsonschema::Validator,
    /// Every property name the schema declares. Only these may appear in an
    /// error; any other key came from the reply.
    known_names: HashSet<String>,
}

impl OutputCheck {
    /// Compiles `schema`. A broken schema is our bug, not the agent's, so it
    /// fails before the CLI is started.
    pub fn new(schema: &serde_json::Value) -> Result<Self, AgentError> {
        let validator =
            jsonschema::validator_for(schema).map_err(|e| AgentError::CouldNotStart {
                reason: format!("the output schema is invalid: {e}"),
            })?;
        let mut known_names = HashSet::new();
        collect_property_names(schema, &mut known_names);
        Ok(Self {
            validator,
            known_names,
        })
    }

    /// Passes `value` through if it matches the schema.
    ///
    /// Nothing from the reply is quoted in an error, since the reply comes from
    /// the transcript: not the values (jsonschema's masked messages), not
    /// unexpected key names (counted instead), and not path segments the
    /// schema does not declare (shown as `*`).
    pub fn check(&self, value: serde_json::Value) -> Result<serde_json::Value, AgentError> {
        let errors: Vec<String> = self
            .validator
            .iter_errors(&value)
            .take(MAX_SCHEMA_ERRORS)
            .map(|e| {
                let at = self.safe_path(e.instance_path().as_str());
                format!("at {at}: {}", safe_message(&e))
            })
            .collect();
        if errors.is_empty() {
            Ok(value)
        } else {
            Err(AgentError::SchemaMismatch { errors })
        }
    }

    /// `pointer` (a JSON Pointer into the reply) with every segment that is
    /// neither an array index nor a declared property name replaced by `*`.
    fn safe_path(&self, pointer: &str) -> String {
        if pointer.is_empty() {
            return "/".to_owned();
        }
        pointer
            .split('/')
            .skip(1)
            .map(|segment| {
                let name = segment.replace("~1", "/").replace("~0", "~");
                if segment.bytes().all(|b| b.is_ascii_digit()) || self.known_names.contains(&name) {
                    segment
                } else {
                    HIDDEN_SEGMENT
                }
            })
            .fold(String::new(), |path, segment| path + "/" + segment)
    }
}

/// The error text, minus anything the reply chose.
///
/// jsonschema's masked mode hides values but still lists unexpected property
/// names, and prints `propertyNames` errors unmasked. Those three kinds get a
/// count instead.
fn safe_message(error: &jsonschema::ValidationError<'_>) -> String {
    match error.kind() {
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            unexpected_properties(unexpected.len())
        }
        ValidationErrorKind::PropertyNames { .. } => "a property name is not allowed".to_owned(),
        _ => error.masked().to_string(),
    }
}

fn unexpected_properties(count: usize) -> String {
    match count {
        1 => "1 unexpected property".to_owned(),
        n => format!("{n} unexpected properties"),
    }
}

/// Adds every key of every `properties` object in `schema` to `names`.
fn collect_property_names(schema: &serde_json::Value, names: &mut HashSet<String>) {
    match schema {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Object(properties)) = map.get("properties") {
                names.extend(properties.keys().cloned());
            }
            for child in map.values() {
                collect_property_names(child, names);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_property_names(child, names);
            }
        }
        _ => {}
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

    fn mismatch(schema: &serde_json::Value, reply: serde_json::Value) -> String {
        let check = OutputCheck::new(schema).unwrap();
        let Err(AgentError::SchemaMismatch { errors }) = check.check(reply) else {
            panic!("expected a mismatch")
        };
        errors.join("\n")
    }

    #[test]
    fn missing_and_extra_fields_are_both_reported() {
        let all = mismatch(&schema(), json!({ "extra": 1 }));
        assert!(all.contains("\"summary\" is a required property"), "{all}");
        assert!(all.contains("1 unexpected property"), "{all}");
    }

    #[test]
    fn an_unexpected_key_name_is_counted_not_quoted() {
        let reply = json!({
            "summary": "s",
            "tasks": [],
            "Bob said layoffs Friday": 1,
            "SECRET too": 2,
        });
        let all = mismatch(&schema(), reply);
        assert!(all.contains("2 unexpected properties"), "{all}");
        assert!(!all.contains("layoffs"), "{all}");
        assert!(!all.contains("SECRET"), "{all}");
    }

    #[test]
    fn a_bad_property_name_is_not_quoted() {
        let schema = json!({ "type": "object", "propertyNames": { "maxLength": 3 } });
        let all = mismatch(&schema, json!({ "SECRET KEY": 1 }));
        assert!(all.contains("a property name is not allowed"), "{all}");
        assert!(!all.contains("SECRET"), "{all}");
    }

    #[test]
    fn a_path_through_a_key_the_schema_does_not_name_is_hidden() {
        let schema = json!({
            "type": "object",
            "additionalProperties": { "type": "integer" },
            "properties": { "count": { "type": "integer" } }
        });
        let all = mismatch(&schema, json!({ "count": "x", "SECRET KEY": "y" }));
        assert!(all.contains("at /count:"), "{all}");
        assert!(all.contains("at /*:"), "{all}");
        assert!(!all.contains("SECRET"), "{all}");
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
