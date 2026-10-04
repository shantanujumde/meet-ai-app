//! A second, kill-on-close Job Object around every agent CLI on Windows
//! (TUR-54).
//!
//! process-wrap 10's std `JobObject` makes its job without
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so if the app dies without running
//! `Drop` (a crash, Task Manager), the job's processes would live on. Here the
//! child is also put in a job of our own with that limit: when the last handle
//! to it closes, which Windows does for a process that dies, everything in it
//! is killed. Jobs nest since Windows 8, so the child sits in both.
//!
//! The child is started suspended (`CREATE_SUSPENDED`, which tells
//! process-wrap not to resume it), put in this job, and only then resumed, so
//! nothing it starts can slip out before it is in.

// Adapted from github.com/watchexec/process-wrap/src/windows.rs @ ca45003a831ac125e6673b8759430e5ef33cc1db (Apache-2.0)
// (`make_job_object` and `resume_threads`; process-wrap is Apache-2.0 OR MIT,
// and these two functions came to it from watchexec under Apache-2.0 only.)

use std::io::{Error, Result};
use std::os::windows::io::AsRawHandle;

use process_wrap::std::ChildWrapper;
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    GetProcessId, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::HRESULT;

/// A Win32 handle closed on drop.
#[derive(Debug)]
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The kill-on-close job. Closing it (dropping this, or the app dying) kills
/// whatever is still in it.
#[derive(Debug)]
pub(crate) struct TreeGuard {
    _job: Owned,
}

// SAFETY: a job handle may be used and closed from any thread.
unsafe impl Send for TreeGuard {}
unsafe impl Sync for TreeGuard {}

/// Puts the suspended `child` in a new kill-on-close job, then resumes it.
pub(crate) fn guard_tree(child: &dyn ChildWrapper) -> Result<TreeGuard> {
    let process = child
        .process_handle()
        .ok_or_else(|| Error::other("the child has no process handle"))?;
    let process = HANDLE(process.as_raw_handle());

    // SAFETY: plain Win32 calls on handles we own or borrow for this call.
    let job = Owned(unsafe { CreateJobObjectW(None, None) }.map_err(Error::other)?);
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = u32::try_from(std::mem::size_of_val(&info)).map_err(Error::other)?;
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&info).cast(),
            size,
        )
    }
    .map_err(Error::other)?;
    unsafe { AssignProcessToJobObject(job.0, process) }.map_err(Error::other)?;
    resume_threads(process)?;
    Ok(TreeGuard { _job: job })
}

/// Resumes every thread of the suspended process. std keeps the main thread's
/// handle to itself, so the threads are found through a ToolHelp snapshot.
fn resume_threads(process: HANDLE) -> Result<()> {
    // SAFETY: Win32 calls on a handle borrowed for this call and on handles
    // this function owns.
    let pid = unsafe { GetProcessId(process) };
    if pid == 0 {
        return Err(Error::last_os_error());
    }
    let snapshot =
        Owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(Error::other)?);
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).map_err(Error::other)?,
        ..Default::default()
    };
    unsafe { Thread32First(snapshot.0, &mut entry) }.map_err(Error::other)?;
    let mut resumed = false;
    loop {
        if entry.th32OwnerProcessID == pid {
            let thread = Owned(
                unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                    .map_err(Error::other)?,
            );
            let previous = unsafe { ResumeThread(thread.0) };
            if previous == u32::MAX {
                return Err(Error::last_os_error());
            }
            resumed |= previous > 0;
        }
        match unsafe { Thread32Next(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(e) if e.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => break,
            Err(e) => return Err(Error::other(e)),
        }
    }
    if resumed {
        Ok(())
    } else {
        Err(Error::other("no suspended thread of the child was found"))
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::process::ProcessTree;

    fn is_gone(pid: u32) -> bool {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let pid = Pid::from_u32(pid);
        let mut system = System::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
            if system.process(pid).is_none() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// The app dying without running any `Drop` still kills the child and
    /// its grandchild: closing the kill-on-close job is all it takes.
    #[test]
    fn closing_the_job_kills_the_tree_without_a_kill_call() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let mut command = Command::new(test_support::fake_cli_path());
        command
            .env("FAKE_GRANDCHILD_PID_FILE", &pid_file)
            .env("FAKE_SLEEP", "30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let tree = ProcessTree::spawn(command).unwrap();
        let child = tree.id();
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read_to_string(&pid_file).map_or(true, |s| s.is_empty())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let grandchild: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        let guard = tree.crash().unwrap();
        assert!(!is_gone(child), "nothing has killed the child yet");
        drop(guard);
        assert!(is_gone(child), "child {child} is still running");
        assert!(
            is_gone(grandchild),
            "grandchild {grandchild} is still running"
        );
    }
}
