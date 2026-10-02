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

/// Turns `</tag` into `<\/tag` for each of `tags`, ignoring case and any
/// spaces after the `/`. Other text is left exactly as it was.
///
/// The prompt modules run the meeting's own text through this before
/// rendering, so someone on the call cannot end a data block early and have
/// the rest read as instructions.
pub(crate) fn neutralize(text: &str, tags: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("</") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let name = after.trim_start();
        let closes_a_block = tags.iter().any(|tag| {
            name.get(..tag.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(tag))
        });
        out.push_str(if closes_a_block { "<\\/" } else { "</" });
        rest = after;
    }
    out.push_str(rest);
    out
}
