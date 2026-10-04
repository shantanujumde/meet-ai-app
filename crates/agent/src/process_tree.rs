//! [`ProcessTree`]: a child process and everything it starts, stopped as one
//! (TUR-54). Split out of `process.rs`, which runs agent CLIs on top of it.

use std::io;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};

use process_wrap::std::{ChildWrapper, CommandWrap};

/// A child started with everything it starts kept together, so one kill
/// stops the lot: a process group it leads on unix, a Job Object on Windows
/// (`crate::platform::wrap_tree`). Built on process-wrap.
///
/// Dropped before its child was seen to exit (an early return, a panic), it
/// kills the tree. After a normal exit, drop leaves anything still running
/// alone, as a plain `Child` does; call [`ProcessTree::kill_tree`] for that.
///
/// For any child the app starts that may start children of its own: agent
/// CLIs here, and user hooks (TUR-63).
#[derive(Debug)]
pub struct ProcessTree {
    child: Box<dyn ChildWrapper>,
    exited: bool,
    /// Windows: the kill-on-close job, so the tree dies with the app even
    /// without `Drop`. Dropped after this type's own `Drop` stop. `None` only
    /// in tests.
    _guard: Option<crate::platform::TreeGuard>,
}

impl ProcessTree {
    /// Spawns `command` (its stdio as the caller set it) as a new tree.
    pub fn spawn(command: Command) -> io::Result<Self> {
        let mut command = CommandWrap::from(command);
        crate::platform::wrap_tree(&mut command);
        let mut child = command.spawn()?;
        let guard = match crate::platform::guard_tree(child.as_ref()) {
            Ok(guard) => guard,
            Err(e) => {
                let _ = child.kill();
                return Err(e);
            }
        };
        Ok(Self {
            child,
            exited: false,
            _guard: Some(guard),
        })
    }

    /// The child's process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// The child's stdin, if it was piped and not taken yet.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin().take()
    }

    /// The child's stdout, if it was piped and not taken yet.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout().take()
    }

    /// The child's stderr, if it was piped and not taken yet.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr().take()
    }

    /// The child's exit status if it has exited, without blocking. Reaps it.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.exited |= status.is_some();
        Ok(status)
    }

    /// Kills every process still in the tree, without waiting. Safe after
    /// the child itself was reaped: the group (unix) or job (Windows) keeps
    /// the rest reachable, and an empty one is a no-op.
    pub fn kill_tree(&mut self) {
        if let Err(e) = self.child.start_kill() {
            // An empty group or a job with nothing left: nothing to kill.
            tracing::trace!("process tree kill: {e}");
        }
    }

    /// Kills the tree, then reaps the child.
    pub fn stop(&mut self) {
        self.kill_tree();
        if self.child.wait().is_ok() {
            self.exited = true;
        }
    }
}

#[cfg(test)]
impl ProcessTree {
    /// Acts as if the app died here: no kill and no reap, only the OS
    /// closing our handles. Returns the guard, whose drop is that close.
    #[allow(
        dead_code,
        reason = "only the Windows test in platform/windows_job.rs uses it"
    )]
    pub(crate) fn crash(mut self) -> Option<crate::platform::TreeGuard> {
        self.exited = true;
        self._guard.take()
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if !self.exited {
            self.stop();
        }
    }
}
