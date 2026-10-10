use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use agent::AgentError;

use super::*;

fn settings(pick: Pick, binary_path: Option<PathBuf>) -> AgentConfig {
    AgentConfig {
        harness: pick,
        binary_path,
        ..AgentConfig::default()
    }
}

/// One error of each kind.
pub(crate) fn one_of_each() -> Vec<AgentError> {
    vec![
        AgentError::NotInstalled {
            harness: "Codex".into(),
        },
        AgentError::NotSignedIn {
            harness: "Claude Code".into(),
        },
        AgentError::TimedOut {
            after: Duration::from_secs(300),
        },
        AgentError::Cancelled,
        AgentError::CliFailed {
            status: Some(2),
            stderr: "bad flag".into(),
        },
        AgentError::InvalidJson {
            reason: "expected value".into(),
        },
        AgentError::SchemaMismatch {
            errors: vec!["/summary: missing".into()],
        },
        AgentError::CouldNotStart {
            reason: "no folder".into(),
        },
    ]
}

#[test]
fn every_kind_has_its_own_key() {
    let keys: HashSet<&str> = ErrorKind::ALL.iter().map(|kind| kind.key()).collect();
    assert_eq!(keys.len(), ErrorKind::ALL.len());
    let mapped: Vec<ErrorKind> = one_of_each().iter().map(ErrorKind::of).collect();
    assert_eq!(mapped, ErrorKind::ALL);
}

#[test]
fn no_agent_picked_is_refused_without_looking() {
    assert!(matches!(
        harness_for(&settings(Pick::None, None), false),
        Err(NotFound::NoAgent)
    ));
    assert!(matches!(
        harness_for(&settings(Pick::None, None), true),
        Err(NotFound::NoAgent)
    ));
}

#[test]
fn a_cli_that_is_not_there_is_not_installed_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("claude");
    let Err(NotFound::Agent { error, sign_in }) =
        harness_for(&settings(Pick::ClaudeCode, Some(missing)), false)
    else {
        panic!("a missing CLI was found");
    };
    assert!(
        matches!(&error, AgentError::NotInstalled { harness } if harness == "Claude Code"),
        "{error:?}"
    );
    assert_eq!(sign_in, "claude auth login");
}

#[test]
fn a_found_cli_runs_from_where_it_is_and_signs_in_as_setup_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let claude = test_support::FakeCli::install(dir.path(), "claude");
    let found = harness_for(
        &settings(Pick::ClaudeCode, Some(claude.path().to_path_buf())),
        false,
    )
    .unwrap_or_else(|_| panic!("the fake claude was not found"));
    assert_eq!(found.harness.id(), agent::claude::ID);
    assert_eq!(
        found.sign_in,
        view::sign_in_command(AgentCliId::ClaudeCode, Some(claude.path()))
    );

    // Codex inside ChatGPT.app: Setup shows its full path on macOS, and so
    // does every runner now.
    let bundle = dir.path().join("ChatGPT.app/Contents/Resources");
    let codex = test_support::FakeCli::install(&bundle, "codex");
    let found = harness_for(
        &settings(Pick::Codex, Some(codex.path().to_path_buf())),
        false,
    )
    .unwrap_or_else(|_| panic!("the fake codex was not found"));
    assert_eq!(found.harness.id(), agent::codex::ID);
    assert_eq!(
        found.sign_in,
        view::sign_in_command(AgentCliId::Codex, Some(codex.path()))
    );
}

#[test]
fn harness_ids_name_their_cli() {
    assert_eq!(display_name(agent::claude::ID), Some("Claude Code"));
    assert_eq!(display_name(agent::codex::ID), Some("Codex"));
    assert_eq!(display_name("fake"), None);
    assert_eq!(
        bare_sign_in(agent::claude::ID).as_deref(),
        Some("claude auth login")
    );
    assert_eq!(bare_sign_in("fake"), None);
    assert_eq!(bare_sign_in("Codex").as_deref(), Some("codex login"));
}

#[test]
fn a_signed_out_cli_s_sentence_gives_the_command() {
    let error = agent_error(
        AgentError::NotSignedIn {
            harness: "Codex".into(),
        },
        Some("/Applications/ChatGPT.app/Contents/Resources/codex login"),
    );
    assert_eq!(error.kind, "agent-not-signed-in");
    assert_eq!(
        error.message,
        "Codex is not signed in. Open a terminal and run \
         /Applications/ChatGPT.app/Contents/Resources/codex login, then try again."
    );
    // Without one, the error's own sentence.
    let plain = agent_error(
        AgentError::NotSignedIn {
            harness: "Fake".into(),
        },
        None,
    );
    assert!(plain.message.contains("not signed in"), "{}", plain.message);
}
