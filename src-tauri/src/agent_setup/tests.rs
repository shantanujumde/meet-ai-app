use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use agent::fake::{FakeBehavior, FakeHarness};
use agent::{AgentError, Install};
use serde_json::json;

use super::test_run::{SAMPLE_TRANSCRIPT, agent_error, found, run_sample, sample_prompt};
use super::view::{binary_paths, cli_view, sign_in_command_for};
use super::*;
use crate::platform::SignInShell;

fn choice(harness: AgentHarness, model: &str, binary_path: Option<&str>) -> AgentChoice {
    AgentChoice {
        harness,
        model: model.to_owned(),
        binary_path: binary_path.map(str::to_owned),
    }
}

fn install(path: &str, signed_in: bool) -> Install {
    Install {
        path: path.into(),
        version: Some("2.1.0 (Claude Code)".into()),
        signed_in,
    }
}

fn temp_root(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "meet-ai-agent-setup-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn good_notes() -> serde_json::Value {
    json!({
        "summary": "The beta ships Friday.",
        "decisions": ["Ship the beta Friday"],
        "open_questions": ["Do we need legal sign-off?"],
        "tasks": [{
            "title": "Write the release notes",
            "details": "For the beta.",
            "owner": "Ben",
            "due": "Thursday",
            "transcript_ref": "00:00:09"
        }]
    })
}

fn run_fake(behavior: FakeBehavior, timeout: Duration) -> Result<AgentTestResult, UiError> {
    run_sample(
        &FakeHarness::new(behavior),
        Some("fake-small"),
        timeout,
        std::env::temp_dir(),
        None,
    )
}

// --- wire shapes -------------------------------------------------------------

#[test]
fn the_wire_names_match_the_contract() {
    let value = serde_json::to_value(choice(AgentHarness::ClaudeCode, "opus", None)).unwrap();
    assert_eq!(
        value,
        json!({ "harness": "claude-code", "model": "opus", "binaryPath": null })
    );
    let back: AgentChoice =
        serde_json::from_value(json!({ "harness": "none", "model": "", "binaryPath": "/x" }))
            .unwrap();
    assert_eq!(back, choice(AgentHarness::None, "", Some("/x")));

    let cli = cli_view(
        AgentCliId::ClaudeCode,
        Some(install("/usr/local/bin/claude", false)),
        vec![],
    );
    let value = serde_json::to_value(cli).unwrap();
    assert_eq!(value["id"], "claude-code");
    assert_eq!(value["state"], "signed-out");
    assert_eq!(value["signInCommand"], "claude auth login");
    assert_eq!(value["canTest"], true);
    assert!(value.get("cliDefault").is_some(), "{value}");
    assert!(value.get("defaultModel").is_none(), "{value}");
}

#[test]
fn a_model_crosses_as_name_label_and_note() {
    let cli = cli_view(AgentCliId::ClaudeCode, None, vec!["haiku".into()]);
    let value = serde_json::to_value(&cli.models).unwrap();
    assert_eq!(
        value,
        json!([{ "name": "haiku", "label": "Haiku", "note": "fastest, cheapest" }])
    );
}

#[test]
fn harness_converts_both_ways() {
    for harness in Harness::ALL {
        assert_eq!(Harness::from(AgentHarness::from(harness)), harness);
    }
}

// --- the rows the screen lists ----------------------------------------------

#[test]
fn a_missing_claude_code_has_no_path_and_cannot_be_tested() {
    let cli = cli_view(AgentCliId::ClaudeCode, None, vec!["opus".into()]);
    assert_eq!(cli.name, "Claude Code");
    assert_eq!(cli.provider, "Anthropic");
    assert_eq!(cli.state, AgentCliState::Missing);
    assert_eq!(cli.path, None);
    assert_eq!(cli.version, None);
    assert_eq!(cli.sign_in_command, "claude auth login");
    // No model of meet-ai's: the CLI picks (A14).
    assert_eq!(cli.cli_default, None);
    assert!(!cli.can_test);
}

fn names(cli: &AgentCli) -> Vec<&str> {
    cli.models.iter().map(|model| model.name.as_str()).collect()
}

#[test]
fn a_signed_in_claude_code_is_ready() {
    let cli = cli_view(
        AgentCliId::ClaudeCode,
        Some(install("/opt/homebrew/bin/claude", true)),
        agent::Harness::models(&agent::ClaudeHarness::new()),
    );
    assert_eq!(cli.state, AgentCliState::Ready);
    assert_eq!(cli.path.as_deref(), Some("/opt/homebrew/bin/claude"));
    assert_eq!(cli.version.as_deref(), Some("2.1.0 (Claude Code)"));
    // Every model in models.json, Sonnet and Haiku first, each with a label.
    assert_eq!(
        names(&cli),
        [
            "sonnet",
            "haiku",
            "opus",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-haiku-4-5",
            "claude-fable-5-1"
        ]
    );
    assert_eq!(cli.models[0].label, "Sonnet");
    assert!(cli.can_test);
}

#[test]
fn codex_s_own_list_wins_and_meet_ai_s_list_stands_in_when_it_is_empty() {
    let own = super::view::codex_models(vec!["gpt-new".into(), "gpt-5.6-terra".into()]);
    assert_eq!(own, ["gpt-new", "gpt-5.6-terra"]);
    let fallback = super::view::codex_models(Vec::new());
    assert_eq!(
        fallback,
        [
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.2"
        ]
    );
}

#[test]
fn codex_names_openai_and_has_no_default_model() {
    let cli = cli_view(
        AgentCliId::Codex,
        Some(install("/usr/local/bin/codex", true)),
        vec!["gpt-5.6-terra".into()],
    );
    assert_eq!(cli.name, "Codex");
    assert_eq!(cli.provider, "OpenAI");
    assert_eq!(cli.state, AgentCliState::Ready);
    assert_eq!(cli.cli_default, None);
    assert_eq!(names(&cli), ["gpt-5.6-terra"]);
    assert_eq!(cli.sign_in_command, "codex login");
    assert!(cli.can_test);

    let missing = cli_view(AgentCliId::Codex, None, vec![]);
    assert_eq!(missing.state, AgentCliState::Missing);
    assert!(!missing.can_test);
}

#[test]
fn a_copy_inside_an_app_bundle_signs_in_by_its_full_path() {
    let bundled = Path::new("/Applications/ChatGPT.app/Contents/Resources/codex");
    assert_eq!(
        mac_sign_in(AgentCliId::Codex, Some(bundled)),
        "/Applications/ChatGPT.app/Contents/Resources/codex login"
    );

    let spaced = Path::new(
        "/Users/a/Library/Application Support/Claude/claude-code/2.1.0/claude.app/Contents/MacOS/claude",
    );
    assert_eq!(
        mac_sign_in(AgentCliId::ClaudeCode, Some(spaced)),
        "'/Users/a/Library/Application Support/Claude/claude-code/2.1.0/claude.app/Contents/MacOS/claude' auth login"
    );

    let quote = Path::new("/Users/o'neil/My Apps/Codex.app/Contents/Resources/codex");
    assert_eq!(
        mac_sign_in(AgentCliId::Codex, Some(quote)),
        r"'/Users/o'\''neil/My Apps/Codex.app/Contents/Resources/codex' login"
    );
}

#[test]
fn a_copy_on_the_path_signs_in_by_its_bare_name() {
    let plain = Path::new("/Users/a b/.local/bin/claude");
    assert_eq!(
        mac_sign_in(AgentCliId::ClaudeCode, Some(plain)),
        "claude auth login"
    );
    // A file called `x.app` is not a bundle around the binary.
    let named = Path::new("/usr/local/bin/codex.app");
    assert_eq!(mac_sign_in(AgentCliId::Codex, Some(named)), "codex login");
}

/// The macOS Terminal command, as before the Windows and Linux port.
fn mac_sign_in(id: AgentCliId, path: Option<&Path>) -> String {
    sign_in_command_for(SignInShell::PosixAppBundles, id, path, None)
}

#[test]
fn linux_always_signs_in_by_the_bare_name() {
    for path in [
        "/home/u/.npm-global/bin/claude",
        "/home/u/.local/bin/claude",
        "/home/linuxbrew/.linuxbrew/bin/claude",
    ] {
        assert_eq!(
            sign_in_command_for(
                SignInShell::Posix,
                AgentCliId::ClaudeCode,
                Some(Path::new(path)),
                None
            ),
            "claude auth login"
        );
    }
}

#[test]
fn powershell_uses_the_bare_name_when_its_folder_is_on_path() {
    let npm = std::env::temp_dir().join("npm");
    let cmd = npm.join("claude.cmd");
    // Another case and a trailing separator still match: Windows folder
    // names are case-insensitive.
    let listed = format!("{}{}", npm.display(), std::path::MAIN_SEPARATOR).to_uppercase();
    let path_var =
        std::env::join_paths([std::env::temp_dir().join("other"), listed.into()]).unwrap();
    assert_eq!(
        sign_in_command_for(
            SignInShell::PowerShell,
            AgentCliId::ClaudeCode,
            Some(&cmd),
            Some(&path_var)
        ),
        "claude auth login"
    );
    assert_eq!(
        sign_in_command_for(SignInShell::PowerShell, AgentCliId::Codex, None, None),
        "codex login"
    );
}

#[test]
fn powershell_runs_a_copy_off_path_by_its_full_path() {
    let plain = std::env::temp_dir().join("bin").join("claude.exe");
    let shown = sign_in_command_for(
        SignInShell::PowerShell,
        AgentCliId::ClaudeCode,
        Some(&plain),
        None,
    );
    assert!(shown.contains(&plain.display().to_string()), "{shown}");
    assert!(shown.ends_with(" auth login"), "{shown}");

    // Spaces and quotes: single-quoted, `'` doubled, behind `&`.
    let spaced = std::env::temp_dir()
        .join("My Tools")
        .join("o'neil")
        .join("codex.exe");
    assert_eq!(
        sign_in_command_for(
            SignInShell::PowerShell,
            AgentCliId::Codex,
            Some(&spaced),
            None
        ),
        format!(
            "& '{}' login",
            spaced.display().to_string().replace('\'', "''")
        )
    );
}

#[test]
fn the_binary_path_goes_only_to_the_picked_cli() {
    let path = Some(PathBuf::from("/opt/claude"));
    let pick = choice(AgentHarness::ClaudeCode, "opus", Some("/opt/claude"));
    assert_eq!(binary_paths(&pick), (path.clone(), None));

    let pick = choice(AgentHarness::Codex, "", Some("/opt/claude"));
    assert_eq!(binary_paths(&pick), (None, path));

    let pick = choice(AgentHarness::None, "", Some("/opt/claude"));
    assert_eq!(binary_paths(&pick), (None, None));

    let pick = choice(AgentHarness::ClaudeCode, "opus", Some("   "));
    assert_eq!(binary_paths(&pick), (None, None));
}

// --- saving --------------------------------------------------------------------

#[test]
fn saving_keeps_auto_run_and_the_time_limit() {
    let current = AgentConfig {
        auto_run: false,
        timeout_sec: 42,
        ..AgentConfig::default()
    };
    let pick = choice(
        AgentHarness::Codex,
        "  gpt-5.6-terra ",
        Some(" /opt/codex "),
    );
    let saved = merged(&pick, Ok(current)).unwrap();
    assert_eq!(
        saved,
        AgentConfig {
            harness: Harness::Codex,
            model: Some("gpt-5.6-terra".into()),
            binary_path: Some("/opt/codex".into()),
            auto_run: false,
            timeout_sec: 42,
        }
    );
}

#[test]
fn saving_manual_writes_auto_run_false_and_a_reload_keeps_it() {
    let current = AgentConfig {
        harness: Harness::Codex,
        model: Some("gpt-5.6-terra".into()),
        timeout_sec: 42,
        ..AgentConfig::default()
    };
    let raw = config::with_agent("{}", &current).unwrap();

    let manual = with_auto_run(false, config::parse_agent(&raw)).unwrap();
    let raw = config::with_agent(&raw, &manual).unwrap();
    assert!(raw.contains("\"auto_run\": false"), "{raw}");
    let reloaded = config::parse_agent(&raw).unwrap();
    assert_eq!(
        reloaded,
        AgentConfig {
            auto_run: false,
            ..current.clone()
        }
    );

    let auto = with_auto_run(true, Ok(reloaded)).unwrap();
    let raw = config::with_agent(&raw, &auto).unwrap();
    assert_eq!(config::parse_agent(&raw).unwrap(), current);
}

#[test]
fn the_auto_manual_choice_is_not_saved_over_a_bad_config() {
    for error in [
        ConfigError::UnknownHarness("codx".into()),
        ConfigError::Invalid("broken".into()),
    ] {
        assert!(with_auto_run(false, Err(error)).is_err());
    }
}

#[test]
fn a_blank_model_is_saved_as_null_for_every_harness() {
    for harness in [AgentHarness::ClaudeCode, AgentHarness::Codex] {
        let saved = merged(&choice(harness, "  ", Some("")), Ok(AgentConfig::default())).unwrap();
        assert_eq!(saved.model, None, "{harness:?}");
        assert_eq!(saved.binary_path, None);
    }

    // Picking Default over a stored opus stores null.
    let opus = AgentConfig {
        model: Some("opus".into()),
        ..AgentConfig::default()
    };
    assert_eq!(AgentChoice::from_config(&opus).model, "opus");
    let saved = merged(&choice(AgentHarness::ClaudeCode, "", None), Ok(opus)).unwrap();
    assert_eq!(saved.model, None);

    let saved = merged(
        &choice(AgentHarness::None, " sonnet ", None),
        Ok(AgentConfig::default()),
    )
    .unwrap();
    assert_eq!(saved.model.as_deref(), Some("sonnet"));
}

#[test]
fn a_fresh_config_shows_a_blank_model() {
    assert_eq!(AgentChoice::from_config(&AgentConfig::default()).model, "");
}

#[test]
fn saving_over_an_unknown_harness_uses_the_defaults() {
    let saved = merged(
        &choice(AgentHarness::ClaudeCode, "sonnet", None),
        Err(ConfigError::UnknownHarness("codx".into())),
    )
    .unwrap();
    let defaults = AgentConfig::default();
    assert_eq!(saved.harness, Harness::ClaudeCode);
    assert_eq!(saved.model.as_deref(), Some("sonnet"));
    assert_eq!(saved.auto_run, defaults.auto_run);
    assert_eq!(saved.timeout_sec, defaults.timeout_sec);
}

#[test]
fn saving_over_a_broken_config_is_refused() {
    let error = merged(
        &choice(AgentHarness::ClaudeCode, "opus", None),
        Err(ConfigError::Invalid(
            "agent.timeout_sec must be whole".into(),
        )),
    )
    .unwrap_err();
    let error = UiError::from(error);
    assert_eq!((error.domain, error.kind), ("app", "invalid-config"));
}

#[test]
fn the_choice_shown_is_the_config_s_agent_section() {
    let agent = AgentConfig {
        harness: Harness::None,
        model: Some("haiku".into()),
        binary_path: Some("/opt/claude".into()),
        ..AgentConfig::default()
    };
    assert_eq!(
        AgentChoice::from_config(&agent),
        choice(AgentHarness::None, "haiku", Some("/opt/claude"))
    );
}

// --- the test run --------------------------------------------------------------

#[test]
fn the_sample_prompt_carries_the_three_lines() {
    let prompt = sample_prompt(None).unwrap();
    for line in SAMPLE_TRANSCRIPT.lines() {
        assert!(prompt.contains(line), "missing {line:?}");
    }
    // The agent version: JSON back, no files to write.
    assert!(!prompt.contains("TICK-"), "{prompt}");
}

#[test]
fn the_sample_prompt_uses_the_user_s_own_template() {
    let root = temp_root("template");
    let prompts = root.join(".app").join("prompts");
    std::fs::create_dir_all(&prompts).unwrap();
    std::fs::write(prompts.join("wrap-up.md"), "MINE {{ transcript }}").unwrap();

    let prompt = sample_prompt(Some(&root)).unwrap();
    assert!(prompt.starts_with("MINE [00:00:01] Ana:"), "{prompt}");

    // No saved template under the root: the built-in one.
    let empty = temp_root("no-template");
    assert_eq!(
        sample_prompt(Some(&empty)).unwrap().lines().next(),
        sample_prompt(None).unwrap().lines().next()
    );
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(empty).ok();
}

#[test]
fn a_good_reply_becomes_the_test_result() {
    crate::platform::skip_without_fake_cli!();
    let result = run_fake(FakeBehavior::Reply(good_notes()), Duration::from_secs(30)).unwrap();
    assert_eq!(result.summary, "The beta ships Friday.");
    assert_eq!(result.decisions, ["Ship the beta Friday"]);
    assert_eq!(result.open_questions, ["Do we need legal sign-off?"]);
    assert_eq!(
        result.tasks,
        [AgentTestTask {
            title: "Write the release notes".into(),
            owner: Some("Ben".into()),
            due: Some("Thursday".into()),
        }]
    );
    assert!(result.seconds < 30, "{}", result.seconds);
}

/// A harness that keeps the model each run got, and replies with good notes.
#[derive(Default)]
struct Recording(std::sync::Mutex<Vec<Option<String>>>);

impl agent::Harness for Recording {
    fn id(&self) -> &'static str {
        "recording"
    }
    fn detect(&self) -> Option<Install> {
        None
    }
    fn models(&self) -> Vec<String> {
        Vec::new()
    }
    fn run(&self, job: &agent::Job) -> Result<serde_json::Value, AgentError> {
        self.0.lock().unwrap().push(job.model.clone());
        Ok(good_notes())
    }
}

#[test]
fn the_test_runs_the_picked_model_and_default_passes_none() {
    let harness = Recording::default();
    for model in [Some("haiku"), None, Some("  "), Some(" sonnet ")] {
        run_sample(
            &harness,
            model,
            Duration::from_secs(5),
            std::env::temp_dir(),
            None,
        )
        .unwrap();
    }
    let got = harness.0.lock().unwrap().clone();
    assert_eq!(
        got,
        [Some("haiku".into()), None, None, Some("sonnet".into())]
    );
}

#[test]
fn a_failed_test_names_the_model_it_tried() {
    crate::platform::skip_without_fake_cli!();
    let fail = FakeBehavior::Fail {
        code: 1,
        stderr: "unknown model".into(),
    };
    let error = run_fake(fail.clone(), Duration::from_secs(30)).unwrap_err();
    assert_eq!(error.kind, "agent-failed");
    assert!(
        error.message.ends_with("(model: fake-small)"),
        "{}",
        error.message
    );

    let error = run_sample(
        &FakeHarness::new(fail),
        None,
        Duration::from_secs(30),
        std::env::temp_dir(),
        None,
    )
    .unwrap_err();
    assert!(!error.message.contains("model:"), "{}", error.message);
}

#[test]
fn each_failed_run_has_its_own_kind() {
    crate::platform::skip_without_fake_cli!();
    let cases = [
        (FakeBehavior::NotInstalled, "agent-not-installed"),
        (FakeBehavior::NotSignedIn, "agent-not-signed-in"),
        (
            FakeBehavior::Fail {
                code: 2,
                stderr: "bad flag".into(),
            },
            "agent-failed",
        ),
        (
            FakeBehavior::Stdout("not json".into()),
            "agent-invalid-json",
        ),
        (FakeBehavior::Reply(json!({})), "agent-schema-mismatch"),
    ];
    for (behavior, kind) in cases {
        let error = run_fake(behavior.clone(), Duration::from_secs(30)).unwrap_err();
        assert_eq!((error.domain, error.kind), ("app", kind), "{behavior:?}");
    }
}

#[test]
fn a_run_past_the_time_limit_is_stopped() {
    crate::platform::skip_without_fake_cli!();
    let error = run_fake(
        FakeBehavior::Sleep(Duration::from_secs(10)),
        Duration::from_millis(300),
    )
    .unwrap_err();
    assert_eq!(error.kind, "agent-timed-out");
}

#[test]
fn every_agent_error_maps_to_a_kind_and_keeps_its_sentence() {
    let cases = [
        (AgentError::Cancelled, "agent-cancelled"),
        (
            AgentError::TimedOut {
                after: Duration::from_secs(300),
            },
            "agent-timed-out",
        ),
        (
            AgentError::CouldNotStart {
                reason: "no folder".into(),
            },
            "agent-could-not-start",
        ),
        (
            AgentError::SchemaMismatch {
                errors: vec!["/summary: missing".into()],
            },
            "agent-schema-mismatch",
        ),
    ];
    for (error, kind) in cases {
        let message = error.to_string();
        let ui = agent_error(error);
        assert_eq!((ui.domain, ui.kind), ("app", kind));
        assert_eq!(ui.message, message);
    }
}

#[test]
fn a_cli_that_is_not_found_is_not_installed_by_name() {
    let error = found(AgentCliId::Codex, None).unwrap_err();
    assert_eq!(error.kind, "agent-not-installed");
    assert!(
        error.message.starts_with("Codex is not installed"),
        "{}",
        error.message
    );

    let ok = found(AgentCliId::ClaudeCode, Some(install("/bin/claude", true))).unwrap();
    assert_eq!(ok.path, PathBuf::from("/bin/claude"));
}

#[test]
fn testing_with_no_agent_picked_is_refused_without_looking() {
    let error = super::test_run::run(&choice(AgentHarness::None, "", None)).unwrap_err();
    assert_eq!((error.domain, error.kind), ("app", "agent-none"));
}
