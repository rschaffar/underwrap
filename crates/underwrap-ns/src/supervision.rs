//! Private liveness and signal-state primitives for namespace supervision.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use underwrap_protocol::exec::WRAPPER_PARENT_LIVENESS_FD;

pub(crate) struct LivenessPipe {
    reader: LivenessReader,
    writer: LivenessWriter,
}

pub(crate) struct LivenessReader(OwnedFd);
pub(crate) struct LivenessWriter(OwnedFd);

pub(crate) fn cloexec_pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    // SAFETY: the array has room for both results. On success pipe2 returns two
    // new, uniquely owned descriptors with CLOEXEC set atomically.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        let error = std::io::Error::last_os_error();
        return Err(error);
    }
    // SAFETY: successful pipe2 returned two distinct, open descriptors whose
    // ownership is transferred exactly once to these OwnedFd values.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

impl LivenessPipe {
    pub(crate) fn new() -> Result<Self, String> {
        let (reader, writer) =
            cloexec_pipe().map_err(|error| format!("creating namespace liveness pipe: {error}"))?;
        let reader = LivenessReader(reader);
        let writer = LivenessWriter(writer);
        Ok(Self { reader, writer })
    }

    pub(crate) fn into_host(self) -> LivenessWriter {
        let Self { reader, writer } = self;
        drop(reader);
        writer
    }

    pub(crate) fn into_relay(self) -> LivenessReader {
        let Self { reader, writer } = self;
        drop(writer);
        reader
    }
}

impl LivenessReader {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl LivenessWriter {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// Adopt fd 21 only after fd-20 peer authentication. Pipe anonymity and sole
/// writer ownership are guaranteed by stage-1 construction, not inferable from
/// the reader endpoint itself.
pub(crate) fn adopt_outer_liveness() -> Result<LivenessReader, String> {
    let fd = WRAPPER_PARENT_LIVENESS_FD;
    // SAFETY: all-zero is a valid initialized `stat` value and supplies aligned,
    // writable output storage whose fields are used only after fstat succeeds.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `stat` remains live and exclusively writable for its full size;
    // fstat borrows the fixed descriptor without consuming its ownership.
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(format!(
            "validating parent liveness fd {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err(format!("parent liveness fd {fd} is not a pipe"));
    }
    // SAFETY: F_GETFL/F_GETFD query descriptor state without ownership change.
    let status = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if status < 0 {
        return Err(format!(
            "querying parent liveness fd {fd} status: {}",
            std::io::Error::last_os_error()
        ));
    }
    if status & libc::O_ACCMODE != libc::O_RDONLY
        || status & (libc::O_NONBLOCK | libc::O_ASYNC) != 0
    {
        return Err(format!(
            "parent liveness fd {fd} has invalid status flags {status:#x}"
        ));
    }
    // SAFETY: F_GETFD queries the validated live descriptor without changing ownership.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    // SAFETY: when the query succeeded, F_SETFD updates only descriptor flags on
    // the same live fd; the trusted CLOEXEC bit and syscall return are checked.
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } != 0 {
        return Err(format!(
            "setting CLOEXEC on parent liveness fd {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the fixed descriptor is now uniquely adopted and no other owner
    // exists in this process.
    Ok(LivenessReader(unsafe { OwnedFd::from_raw_fd(fd) }))
}

pub(crate) fn arm_and_check(reader: &LivenessReader) -> Result<(), String> {
    // SAFETY: PR_SET_PDEATHSIG accepts an integer signal; SIGKILL is valid and
    // creates no memory aliasing.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(format!(
            "arming parent-death SIGKILL: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut configured = 0;
    // SAFETY: PR_GET_PDEATHSIG writes one integer to the valid pointer.
    if unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut configured) } != 0 {
        return Err(format!(
            "querying parent-death signal: {}",
            std::io::Error::last_os_error()
        ));
    }
    if configured != libc::SIGKILL {
        return Err(format!(
            "parent-death signal mismatch: expected SIGKILL ({}), observed {configured}",
            libc::SIGKILL
        ));
    }
    poll_parent_alive(reader, 0)
}

/// Wait for at most `timeout_ms` while requiring the sole-writer liveness pipe
/// to remain event-free. Anonymous liveness writers never send data, so every
/// event is fail-closed cancellation rather than an application message.
pub(crate) fn poll_parent_alive(
    reader: &LivenessReader,
    timeout_ms: libc::c_int,
) -> Result<(), String> {
    let mut pollfd = libc::pollfd {
        fd: reader.raw_fd(),
        events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
        revents: 0,
    };
    loop {
        // SAFETY: pollfd points to one initialized entry for the duration of
        // poll. The owned reader remains open across the syscall.
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if result == 0 {
            return Ok(());
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("checking parent liveness: {error}"));
        }
        return Err(format!(
            "parent liveness ended or carried unexpected data ({:#x})",
            pollfd.revents
        ));
    }
}

#[derive(Clone)]
pub(crate) struct SigchldState {
    action: libc::sigaction,
    mask: libc::sigset_t,
}

impl SigchldState {
    pub(crate) fn capture() -> Result<Self, String> {
        // SAFETY: all-zero is a valid initialized `sigaction` and supplies
        // aligned writable storage for the complete kernel query output.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: a null new-action requests a query; `action` is live writable
        // output for the full structure and is used only after success.
        if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0 {
            return Err(format!(
                "querying SIGCHLD disposition: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: all-zero is a valid initialized sigset and supplies aligned
        // writable storage for the complete kernel mask query output.
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: a null input set requests a query; `mask` remains live and
        // exclusively writable for its full size until the checked return.
        if unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask) } != 0 {
            return Err(format!(
                "querying signal mask: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { action, mask })
    }

    pub(crate) fn normalize_for_wait(&self) -> Result<(), String> {
        // SAFETY: all-zero is a valid initialized sigaction; the handler and
        // mask are explicitly completed below before installation.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_DFL;
        // SAFETY: `sa_mask` is aligned writable storage within live `action`.
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
            // SAFETY: `action` now has a default handler and initialized empty
            // mask; sigaction borrows the complete live structure for SIGCHLD.
            || unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } != 0
        {
            return Err(format!(
                "normalizing SIGCHLD: {}",
                std::io::Error::last_os_error()
            ));
        }
        set_signal_blocked(libc::SIGCHLD, false)
    }

    pub(crate) fn restore(&self) -> Result<(), String> {
        // SAFETY: `action` was completely returned by the kernel before fork and
        // remains an initialized, aligned copied value borrowed for restoration.
        if unsafe { libc::sigaction(libc::SIGCHLD, &self.action, std::ptr::null_mut()) } != 0
            // SAFETY: `mask` is the complete initialized kernel-returned thread
            // mask; SIG_SETMASK borrows it and no old-mask output is requested.
            || unsafe { libc::sigprocmask(libc::SIG_SETMASK, &self.mask, std::ptr::null_mut()) }
                != 0
        {
            return Err(format!(
                "restoring inherited SIGCHLD state: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

fn set_signal_blocked(signal: libc::c_int, blocked: bool) -> Result<(), String> {
    // SAFETY: all-zero is a valid initialized sigset value; it remains aligned
    // writable storage for the following sigset operations.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is live, aligned, and exclusively borrowed for initialization.
    if unsafe { libc::sigemptyset(&mut set) } != 0
        // SAFETY: `set` remains initialized and writable; `signal` is a trusted
        // libc constant or validated direct-child signal number.
        || unsafe { libc::sigaddset(&mut set, signal) } != 0
    {
        return Err(format!(
            "building signal mask: {}",
            std::io::Error::last_os_error()
        ));
    }
    let how = if blocked {
        libc::SIG_BLOCK
    } else {
        libc::SIG_UNBLOCK
    };
    // SAFETY: `set` is a complete initialized live mask, `how` is one of two
    // trusted operations, and no old-mask output pointer is requested.
    if unsafe { libc::sigprocmask(how, &set, std::ptr::null_mut()) } != 0 {
        return Err(format!(
            "changing signal mask: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub(crate) fn propagate_signal(signal: libc::c_int) -> ! {
    if signal != libc::SIGKILL && signal != libc::SIGSTOP {
        // SAFETY: all-zero is a valid initialized sigaction; handler and mask are
        // completed below before the structure is installed.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_DFL;
        // SAFETY: `sa_mask` is aligned writable storage within live `action`.
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
            // SAFETY: `action` is now complete; `signal` came from a validated
            // direct-child status and excludes the two unchangeable signals.
            || unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0
        {
            eprintln!(
                "underwrap-ns: resetting signal {signal}: {}",
                std::io::Error::last_os_error()
            );
            std::process::exit(1);
        }
    }
    if let Err(error) = set_signal_blocked(signal, false) {
        eprintln!("underwrap-ns: unblocking signal {signal}: {error}");
        std::process::exit(1);
    }
    // SAFETY: `signal` came from a validated direct-child wait frame and has
    // been reset/unblocked as applicable; raise takes only this scalar value.
    if unsafe { libc::raise(signal) } != 0 {
        eprintln!(
            "underwrap-ns: raising signal {signal}: {}",
            std::io::Error::last_os_error()
        );
    }
    std::process::exit(128 + signal)
}

#[cfg(test)]
mod tests;
