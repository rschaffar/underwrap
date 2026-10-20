//! Direct packaged-child ownership, independent of a Tokio runtime's lifetime.
//!
//! These services do not launch descendants. Reaping their direct child closes
//! their listeners; this is not a general process-tree supervisor.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, Command, ExitStatus, Output};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLEANUP_BUDGET: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(5);

pub(crate) struct PackagedChild {
    child: Child,
    state: Option<Arc<tempfile::TempDir>>,
    status: Option<ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    // Test-only simulated observation failure, after real cleanup succeeds.
    cleanup_observation_error: bool,
}

impl PackagedChild {
    pub(crate) fn spawn(command: &mut Command, state: &Arc<tempfile::TempDir>) -> io::Result<Self> {
        Self::spawn_with(command, state, Self::prepare_output)
    }

    fn spawn_with(
        command: &mut Command,
        state: &Arc<tempfile::TempDir>,
        setup: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<Self> {
        let state = Arc::clone(state);
        let child = command.spawn()?;
        // No fallible operation between acquiring the child and installing its
        // owner, including the independently retained temporary state.
        let mut owner = Self {
            child,
            state: Some(state),
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            cleanup_observation_error: false,
        };
        setup(&mut owner)?;
        Ok(owner)
    }

    fn prepare_output(&mut self) -> io::Result<()> {
        for fd in [
            self.child.stdout.as_ref().map(AsRawFd::as_raw_fd),
            self.child.stderr.as_ref().map(AsRawFd::as_raw_fd),
        ]
        .into_iter()
        .flatten()
        {
            // SAFETY: the child owns this live pipe throughout both calls;
            // changing status flags neither borrows nor transfers ownership.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn take_stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.stdin.take()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
        }
        Ok(self.status)
    }

    pub(crate) fn ensure_running(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.try_wait()?.is_none(),
            "packaged backend exited before readiness"
        );
        Ok(())
    }

    pub(crate) fn kill(&mut self) -> io::Result<()> {
        self.child.stdin.take();
        self.child.kill()
    }

    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        self.kill()
    }

    pub(crate) fn drain_output(&mut self) -> io::Result<()> {
        fn drain(pipe: &mut Option<impl Read>, bytes: &mut Vec<u8>) -> io::Result<()> {
            let Some(reader) = pipe else { return Ok(()) };
            // Bound one poll even if a running service continuously writes.
            for _ in 0..16 {
                let mut buffer = [0; 4096];
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        *pipe = None;
                        return Ok(());
                    }
                    Ok(len) => bytes.extend_from_slice(&buffer[..len]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }
        drain(&mut self.child.stdout, &mut self.stdout)?;
        drain(&mut self.child.stderr, &mut self.stderr)?;
        Ok(())
    }

    fn poll_output(&mut self) -> io::Result<Option<Output>> {
        self.poll_output_with(Self::drain_output)
    }

    fn poll_output_with(
        &mut self,
        drain: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<Option<Output>> {
        drain(self)?;
        if let Some(status) = self.try_wait()?
            && self.child.stdout.is_none()
            && self.child.stderr.is_none()
        {
            return Ok(Some(Output {
                status,
                stdout: std::mem::take(&mut self.stdout),
                stderr: std::mem::take(&mut self.stderr),
            }));
        }
        Ok(None)
    }

    // Borrow, never consume, the owner during output waits. Cancelling this
    // future leaves child, partial output, reap status and state owned together.
    pub(crate) async fn wait_with_output(&mut self) -> io::Result<Output> {
        loop {
            if let Some(output) = self.poll_output()? {
                return Ok(output);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    pub(crate) fn wait_with_output_sync(&mut self) -> io::Result<Output> {
        loop {
            if let Some(output) = self.poll_output()? {
                return Ok(output);
            }
            std::thread::sleep(POLL);
        }
    }

    fn kill_and_reap(&mut self) -> io::Result<()> {
        self.kill_and_reap_with(Self::kill, Self::try_wait, CLEANUP_BUDGET)
    }

    fn kill_and_reap_with(
        &mut self,
        signal: impl FnOnce(&mut Self) -> io::Result<()>,
        mut observe: impl FnMut(&mut Self) -> io::Result<Option<ExitStatus>>,
        budget: Duration,
    ) -> io::Result<()> {
        if matches!(observe(self), Ok(Some(_))) {
            return Ok(());
        }
        // A signal error can race ordinary exit. Only observed reap decides
        // whether cleanup is confirmed, not the success of the signal attempt.
        let _ = signal(self);
        let deadline = Instant::now() + budget;
        loop {
            if matches!(observe(self), Ok(Some(_))) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child reap not confirmed",
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    fn retain_state(&mut self, error: &io::Error) {
        if let Some(state) = self.state.take() {
            // Deliberately keep the Arc alive, including if another child or
            // caller still holds one. Last-owner deletion must not run later.
            let _ = writeln!(
                io::stderr(),
                "packaged child pid {} cleanup unconfirmed ({:?}); retaining fixture state at {}",
                self.id(),
                error.kind(),
                state.path().display()
            );
            std::mem::forget(state);
        }
    }
}

impl Drop for PackagedChild {
    fn drop(&mut self) {
        // Synchronous fallback works during async cancellation/runtime teardown.
        // It is bounded locally but is not hidden in a cancellable async timeout.
        // Never panic here: preserve the original assertion/startup error.
        let cleanup = self.kill_and_reap().and_then(|()| {
            if self.cleanup_observation_error {
                Err(io::Error::other("injected failed cleanup observation"))
            } else {
                Ok(())
            }
        });
        match cleanup {
            Ok(()) => {
                if let Some(state) = &self.state {
                    tests::record_reap(self.id(), state.path());
                }
            }
            Err(error) => self.retain_state(&error),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
