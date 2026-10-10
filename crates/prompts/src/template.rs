//! What every prompt module shares: finding the user's own copy of a
//! template, rendering it the same strict way, and keeping meeting text inside
//! its data blocks.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use meeting_format::layout::app_dir;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};

use crate::{Error, PromptKind};

/// The prompt templates folder's name inside `.app` (SPEC §3.1).
pub const PROMPTS_DIR: &str = "prompts";

/// `<root>/.app/prompts`, where the user's own templates live. `root` is the
/// meetings folder.
pub fn prompts_dir(root: &Path) -> PathBuf {
    app_dir(root).join(PROMPTS_DIR)
}

/// The `kind` template to use for the meetings folder `root`: the user's
/// `<root>/.app/prompts/<name>` if it exists, otherwise `default`.
///
/// A missing file means "use the default". Any other read failure (no
/// permission, not UTF-8, a folder in its place) is [`Error::Io`], so the user
/// learns their edit is being ignored.
pub(crate) fn load(root: &Path, kind: PromptKind, default: &str) -> Result<String, Error> {
    let path = prompts_dir(root).join(kind.template_name());
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(default.to_owned()),
        Err(err) => Err(Error::Io(err)),
    }
}

/// Renders `template` as the `kind` prompt with `context` as its variables.
///
/// Rendering is strict: a variable the template names but `context` does not
/// provide is an error, so a typo in a user's template is reported rather
/// than turning into blank text. Nothing is HTML-escaped; the output is
/// markdown. Every minijinja failure (bad syntax, unknown filter, undefined
/// variable, a failed call) is [`Error::Template`] with minijinja's message,
/// which names the line.
pub(crate) fn render(
    kind: PromptKind,
    template: &str,
    context: impl serde::Serialize,
) -> Result<String, Error> {
    let name = kind.template_name();
    let error = |err: minijinja::Error| Error::Template {
        template: name.to_owned(),
        detail: err.to_string(),
    };
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.add_template(name, template).map_err(error)?;
    env.get_template(name)
        .and_then(|compiled| compiled.render(context))
        .map_err(error)
}

/// Turns `</tag` into `<\/tag` for each of `tags`, ignoring case. Whitespace
/// or zero-width characters between the `<`, the `/` and the name do not
/// hide it (TUR-158): `< /tag` becomes `< \/tag` and `<\u{200b}/tag` becomes
/// `<\u{200b}\/tag`. Other text is left exactly as it was.
///
/// The prompt modules run the meeting's own text through this before
/// rendering, so someone on the call cannot end a data block early and have
/// the rest read as instructions.
pub(crate) fn neutralize(text: &str, tags: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        // Everything up to and including the `<` is kept as it is.
        let after_lt = &rest[at + 1..];
        out.push_str(&rest[..=at]);
        let gap = after_lt.len() - after_lt.trim_start_matches(is_gap).len();
        if let Some(after_slash) = after_lt[gap..].strip_prefix('/')
            && names_a_tag(after_slash.trim_start_matches(is_gap), tags)
        {
            out.push_str(&after_lt[..gap]);
            out.push_str("\\/");
            rest = after_slash;
        } else {
            rest = after_lt;
        }
    }
    out.push_str(rest);
    out
}

/// Space a reader (or a model) skips over inside a tag: any Unicode
/// whitespace, and the zero-width characters that render as nothing.
fn is_gap(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

/// Does `name` start with one of `tags`, ignoring ASCII case?
fn names_a_tag(name: &str, tags: &[&str]) -> bool {
    tags.iter().any(|tag| {
        name.get(..tag.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(tag))
    })
}

#[cfg(test)]
mod tests {
    use super::neutralize;

    const TAGS: [&str; 2] = ["transcript", "notes"];

    fn guard(text: &str) -> String {
        neutralize(text, &TAGS)
    }

    #[test]
    fn the_plain_closing_tag_is_neutralized_as_before() {
        assert_eq!(guard("a </transcript> b"), "a <\\/transcript> b");
        assert_eq!(guard("</ NOTES>"), "<\\/ NOTES>");
    }

    #[test]
    fn whitespace_between_the_angle_and_the_slash_does_not_hide_it() {
        assert_eq!(guard("< /transcript>"), "< \\/transcript>");
        assert_eq!(guard("<\t\n /Transcript>"), "<\t\n \\/Transcript>");
        assert_eq!(guard("< / notes>"), "< \\/ notes>");
    }

    #[test]
    fn zero_width_characters_do_not_hide_it() {
        for zero_width in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'] {
            let before = format!("<{zero_width}/transcript>");
            let after = format!("<{zero_width}\\/transcript>");
            assert_eq!(guard(&before), after, "{zero_width:?} before the slash");
            let before = format!("</{zero_width}transcript>");
            assert_eq!(guard(&before), format!("<\\/{zero_width}transcript>"));
        }
    }

    #[test]
    fn other_text_with_angles_is_left_alone() {
        for text in [
            "1 < 2 and 3 > 2",
            "a < /b> < /tr>",
            "<<",
            "ends with <",
            "ends with < /",
            "< \u{200B}",
            "é< /é",
            "<transcript>",
        ] {
            assert_eq!(guard(text), text, "{text:?}");
        }
    }

    #[test]
    fn a_run_of_angles_still_finds_the_closing_tag() {
        assert_eq!(guard("<< /notes>"), "<< \\/notes>");
        assert_eq!(guard("<</notes>"), "<<\\/notes>");
    }
}
