//! A pretend agent CLI for tests, here and in the app (feature
//! `test-support`).
//!
//! [`FakeHarness`] does not call any model. It runs `fake-cli`, a tiny Rust
//! program from the `test-support` crate (`test_support::fake_cli_path`),
//! through the same [`run_cli`](crate::process::run_cli) path a real CLI goes
//! through, so the fresh working folder, the prompt on stdin, the time limit
//! and Cancel are all the real thing: a run that sleeps past its time limit
//! is really killed.
//!
//! What the program prints is picked by [`FakeBehavior`], handed to it in
//! environment variables. It works the same on every OS, so these tests run
//! on Windows and Linux too.

use std::time::Duration;

use crate::{AgentError, Harness, Install, Job, OutputCheck, parse_json};

/// The harness id, as `Harness::id` returns it and as errors name it.
const ID: &str = "fake";

/// What the fake CLI does when it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeBehavior {
    /// Prints this JSON on stdout and exits 0.
    Reply(serde_json::Value),
    /// Prints this text on stdout as-is and exits 0. For replies that are
    /// not JSON.
    Stdout(String),
    /// Prints `stderr` on stderr and exits with `code`.
    Fail { code: i32, stderr: String },
    /// Waits this long, then prints `{}`. For the time limit and Cancel.
    Sleep(Duration),
    /// Acts as if the CLI is not on this Mac.
    NotInstalled,
    /// Acts as if the CLI is installed but nobody is signed in.
    NotSignedIn,
}

/// A [`Harness`] that runs a canned [`FakeBehavior`] instead of a real agent.
#[derive(Debug, Clone)]
pub struct FakeHarness {
    behavior: FakeBehavior,
    models: Vec<String>,
}

impl FakeHarness {
    /// A fake that does `behavior` on every run, with the models
    /// `fake-small` and `fake-large`.
    pub fn new(behavior: FakeBehavior) -> Self {
        Self {
            behavior,
            models: vec!["fake-small".to_owned(), "fake-large".to_owned()],
        }
    }

    /// Replaces the model list the setup picker sees.
    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    /// The program's environment for this behavior: stdout, stderr, exit code
    /// and sleep time. The behaviors that never start a process return their
    /// error instead.
    fn script_env(&self) -> Result<ScriptEnv, AgentError> {
        let (stdout, stderr, code, sleep) = match &self.behavior {
            FakeBehavior::Reply(value) => (value.to_string(), String::new(), 0, String::new()),
            FakeBehavior::Stdout(text) => (text.clone(), String::new(), 0, String::new()),
            FakeBehavior::Fail { code, stderr } => {
                (String::new(), stderr.clone(), *code, String::new())
            }
            FakeBehavior::Sleep(time) => (
                "{}".to_owned(),
                String::new(),
                0,
                format!("{:.3}", time.as_secs_f64()),
            ),
            FakeBehavior::NotInstalled => {
                return Err(AgentError::NotInstalled {
                    harness: ID.to_owned(),
                });
            }
            FakeBehavior::NotSignedIn => {
                return Err(AgentError::NotSignedIn {
                    harness: ID.to_owned(),
                });
            }
        };
        Ok([
            ("FAKE_STDOUT", stdout),
            ("FAKE_STDERR", stderr),
            ("FAKE_CODE", code.to_string()),
            ("FAKE_SLEEP", sleep),
        ])
    }
}

impl Harness for FakeHarness {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self) -> Option<Install> {
        if self.behavior == FakeBehavior::NotInstalled {
            return None;
        }
        Some(Install {
            path: test_support::fake_cli_path().to_path_buf(),
            version: Some("fake 1.0".into()),
            signed_in: self.behavior != FakeBehavior::NotSignedIn,
        })
    }

    fn models(&self) -> Vec<String> {
        self.models.clone()
    }

    fn run(&self, job: &Job) -> Result<serde_json::Value, AgentError> {
        let env = self.script_env()?;
        let check = OutputCheck::new(&job.schema)?;
        let stdout = run_script(env, job)?;
        check.check(parse_json(&stdout)?)
    }
}

/// The program's environment, as [`FakeHarness::script_env`] builds it.
/// `fake-cli` reads the prompt to its end first (so a long prompt never
/// blocks on a full pipe), then sleeps, prints and exits as these say.
type ScriptEnv = [(&'static str, String); 4];

/// Runs `fake-cli` with `env` in a fresh working folder, which is deleted
/// before this returns, and hands back what it printed on stdout.
fn run_script(env: ScriptEnv, job: &Job) -> Result<String, AgentError> {
    use crate::process;

    let dir = process::fresh_work_dir(job)?;
    let mut command = std::process::Command::new(test_support::fake_cli_path());
    // Only the settings this behavior names, never ones a test left behind.
    command.envs(env.into_iter().filter(|(_, v)| !v.is_empty()));
    let out = process::run_cli(ID, command, job, dir.path())?;
    drop(dir);
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_install_and_sign_in() {
        assert!(
            FakeHarness::new(FakeBehavior::NotInstalled)
                .detect()
                .is_none()
        );

        let install = FakeHarness::new(FakeBehavior::NotSignedIn)
            .detect()
            .unwrap();
        assert!(!install.signed_in);

        let install = FakeHarness::new(FakeBehavior::Stdout("x".into()))
            .detect()
            .unwrap();
        assert!(install.signed_in);
        assert_eq!(install.path, test_support::fake_cli_path());
        assert_eq!(install.version.as_deref(), Some("fake 1.0"));
    }

    #[test]
    fn models_default_and_override() {
        let fake = FakeHarness::new(FakeBehavior::Reply(serde_json::json!({})));
        assert_eq!(fake.id(), "fake");
        assert_eq!(fake.models(), ["fake-small", "fake-large"]);

        let fake = fake.with_models(vec!["only-one".into()]);
        assert_eq!(fake.models(), ["only-one"]);
    }

    #[test]
    fn sleep_time_is_passed_in_fractional_seconds() {
        let env = FakeHarness::new(FakeBehavior::Sleep(Duration::from_millis(1500)))
            .script_env()
            .unwrap();
        assert!(env.contains(&("FAKE_SLEEP", "1.500".to_owned())), "{env:?}");
        assert!(env.contains(&("FAKE_STDOUT", "{}".to_owned())), "{env:?}");
    }
}
