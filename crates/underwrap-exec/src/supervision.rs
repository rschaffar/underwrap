//! Private process-lifetime and signal-state helpers for stage 1.

use anyhow::{Context, Result, bail};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

pub(crate) const RELOCATED_FD_MIN: RawFd = 22;

pub(crate) struct LivenessPipe {
    pub(crate) reader: OwnedFd,
    pub(crate) writer: OwnedFd,
}

impl LivenessPipe {
    pub(crate) fn new() -> Result<Self> {
        let mut fds = [-1; 2];
        // SAFETY: `fds` has room for both descriptors. Successful pipe2 returns
        // two new descriptors, adopted exactly once below.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err(std::io::Error::last_os_error()).context("creating parent liveness pipe");
        }
        // SAFETY: successful pipe2 returned uniquely owned descriptors.
        let reader = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: same ownership invariant for the writer.
        let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        Ok(Self { reader, writer })
    }
}

pub(crate) fn relocate_owned(fd: RawFd, what: &str) -> Result<OwnedFd> {
    // SAFETY: fcntl duplicates an open descriptor without borrowing memory.
    let relocated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, RELOCATED_FD_MIN) };
    if relocated < 0 {
        bail!(
            "relocating {what} fd {fd}: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: F_DUPFD_CLOEXEC returned a new uniquely owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(relocated) })
}

pub(crate) fn map_fixed(source: &OwnedFd, destination: RawFd, what: &str) -> Result<()> {
    // SAFETY: `source` is the live relocated descriptor at or above
    // RELOCATED_FD_MIN, so it differs from the deliberate fixed handoff
    // `destination`. dup2 atomically replaces the destination and clears CLOEXEC.
    if unsafe { libc::dup2(source.as_raw_fd(), destination) } < 0 {
        bail!(
            "mapping {what} to fd {destination}: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct SigchldState {
    action: libc::sigaction,
    mask: libc::sigset_t,
}

impl SigchldState {
    pub(crate) fn capture() -> Result<Self> {
        // SAFETY: zero is a valid initialization for these kernel output
        // structures, and the pointers remain valid for each syscall.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: null new-action queries the current SIGCHLD disposition.
        if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0 {
            return Err(std::io::Error::last_os_error()).context("querying SIGCHLD disposition");
        }
        // SAFETY: zero initializes storage that sigprocmask fills completely.
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: null set queries the calling thread's current mask.
        if unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask) } != 0 {
            return Err(std::io::Error::last_os_error()).context("querying signal mask");
        }
        Ok(Self { action, mask })
    }

    pub(crate) fn normalize_for_wait(&self) -> Result<()> {
        // SAFETY: zero plus an initialized empty sa_mask describes SIG_DFL with
        // no SA_NOCLDWAIT/SA_NOCLDSTOP inheritance for the supervisor.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_DFL;
        // SAFETY: action.sa_mask is valid mutable storage.
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
            return Err(std::io::Error::last_os_error()).context("emptying SIGCHLD action mask");
        }
        // SAFETY: action is fully initialized for sigaction.
        if unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("normalizing SIGCHLD disposition");
        }
        set_signal_blocked(libc::SIGCHLD, false).context("unblocking SIGCHLD")
    }

    pub(crate) fn restore(&self) -> Result<()> {
        // SAFETY: action and mask were returned by the kernel in capture and
        // remain valid copied values for this single-threaded fork stage.
        if unsafe { libc::sigaction(libc::SIGCHLD, &self.action, std::ptr::null_mut()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("restoring SIGCHLD disposition");
        }
        // SAFETY: self.mask is the complete captured thread mask.
        if unsafe { libc::sigprocmask(libc::SIG_SETMASK, &self.mask, std::ptr::null_mut()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("restoring signal mask");
        }
        Ok(())
    }
}

fn set_signal_blocked(signal: libc::c_int, blocked: bool) -> Result<()> {
    // SAFETY: zero initializes a sigset_t before sigemptyset/sigaddset.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: set is valid mutable storage and signal is supplied by trusted code.
    if unsafe { libc::sigemptyset(&mut set) } != 0
        // SAFETY: `set` remains initialized, aligned, and exclusively borrowed;
        // `signal` is a trusted libc signal constant or validated wait result.
        || unsafe { libc::sigaddset(&mut set, signal) } != 0
    {
        return Err(std::io::Error::last_os_error()).context("building signal set");
    }
    let how = if blocked {
        libc::SIG_BLOCK
    } else {
        libc::SIG_UNBLOCK
    };
    // SAFETY: set is initialized and the old mask is not requested.
    if unsafe { libc::sigprocmask(how, &set, std::ptr::null_mut()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("changing signal mask");
    }
    Ok(())
}

pub(crate) fn wait_terminal(pid: libc::pid_t) -> Result<libc::c_int> {
    let mut status = 0;
    loop {
        // SAFETY: status points to writable storage and pid is our fork child.
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result == pid {
            if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                return Ok(status);
            }
            bail!("waitpid returned nonterminal child state {status}");
        }
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if result < 0 {
            return Err(std::io::Error::last_os_error()).context("waiting for underwrap-ns");
        }
        bail!("waitpid returned unexpected pid {result}");
    }
}

pub(crate) fn kill_and_reap(pid: libc::pid_t) {
    // SAFETY: pid is the stage child. ESRCH is harmless; waitpid below still
    // consumes a zombie if one exists.
    if unsafe { libc::kill(pid, libc::SIGKILL) } != 0
        && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        return;
    }
    loop {
        let mut status = 0;
        // SAFETY: status is writable and pid names the one child to reap.
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result == pid
            || (result < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD))
        {
            return;
        }
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    }
}

pub(crate) fn propagate_wait_status(status: libc::c_int) -> ! {
    if libc::WIFEXITED(status) {
        std::process::exit(libc::WEXITSTATUS(status));
    }
    if libc::WIFSIGNALED(status) {
        let signal = libc::WTERMSIG(status);
        if signal != libc::SIGKILL && signal != libc::SIGSTOP {
            // SAFETY: zero plus an empty mask is a complete default sigaction.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = libc::SIG_DFL;
            // SAFETY: action mask is valid storage.
            if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
                // SAFETY: action is fully initialized for this signal.
                || unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0
            {
                eprintln!(
                    "underwrap-exec: resetting signal {signal}: {}",
                    std::io::Error::last_os_error()
                );
                std::process::exit(1);
            }
        }
        if let Err(error) = set_signal_blocked(signal, false) {
            eprintln!("underwrap-exec: unblocking signal {signal}: {error:#}");
            std::process::exit(1);
        }
        // SAFETY: raising a validated wait-status signal to this process exactly
        // reproduces the child's terminal status after reset/unblock.
        if unsafe { libc::raise(signal) } != 0 {
            eprintln!(
                "underwrap-exec: raising signal {signal}: {}",
                std::io::Error::last_os_error()
            );
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(1)
}

#[cfg(test)]
mod tests;
