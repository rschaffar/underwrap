//! Isolated-process hard bounds for fixture witnesses, not async join timeouts.

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct ExternalTest {
    child: Child,
    deadline: Instant,
    reaped: bool,
}

impl ExternalTest {
    pub(crate) fn spawn(name: &str, env: &[(&str, &std::ffi::OsStr)]) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([name, "--exact", "--test-threads=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        for (key, value) in env {
            command.env(key, value);
        }
        Self {
            child: command.spawn().expect("starting isolated fixture witness"),
            deadline: Instant::now() + Duration::from_secs(30),
            reaped: false,
        }
    }

    fn exited_without_reaping(&self) -> std::io::Result<bool> {
        // SAFETY: waitid initializes the writable siginfo, and WNOWAIT keeps the
        // direct child's identity/PID reserved until group cleanup, even if the
        // witness exits before its packaged children. No global SIGCHLD changes.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { info.si_pid() } != 0)
    }

    fn kill_group(&self) {
        // SAFETY: the unreaped direct child reserves this group leader's PID;
        // the negative PID selects only its dedicated process group.
        unsafe { libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL) };
    }

    pub(crate) fn check_running(&mut self) {
        assert!(
            !self.exited_without_reaping().unwrap(),
            "isolated witness exited before handshake"
        );
        assert!(
            Instant::now() < self.deadline,
            "external fixture witness deadline expired"
        );
    }

    pub(crate) fn wait(&mut self) -> ExitStatus {
        loop {
            if self.exited_without_reaping().unwrap() {
                self.kill_group();
                let status = self.child.wait().expect("reaping isolated fixture witness");
                self.reaped = true;
                return status;
            }
            assert!(
                Instant::now() < self.deadline,
                "external fixture witness deadline expired"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for ExternalTest {
    fn drop(&mut self) {
        // The isolated process creates a new group before exec. This fallback
        // also terminates packaged children if the witness itself stops making
        // progress. It is a safety net, not evidence of owner-mediated reap.
        if !self.reaped {
            self.kill_group();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests;
