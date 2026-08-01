use super::*;
use std::os::fd::AsRawFd;

#[test]
fn liveness_pipe_is_cloexec_and_directional() {
    let pipe = LivenessPipe::new().unwrap();
    for fd in [pipe.reader.as_raw_fd(), pipe.writer.as_raw_fd()] {
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
    assert_eq!(
        unsafe { libc::fcntl(pipe.reader.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE,
        libc::O_RDONLY
    );
    assert_eq!(
        unsafe { libc::fcntl(pipe.writer.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE,
        libc::O_WRONLY
    );
}

#[test]
fn relocation_stays_above_all_fixed_handoff_slots() {
    let pipe = LivenessPipe::new().unwrap();
    let relocated = relocate_owned(pipe.reader.as_raw_fd(), "test").unwrap();
    assert!(relocated.as_raw_fd() >= RELOCATED_FD_MIN);
    assert_ne!(
        relocated.as_raw_fd(),
        underwrap_protocol::exec::EXEC_BUNDLE_FD
    );
    assert_ne!(
        relocated.as_raw_fd(),
        underwrap_protocol::exec::WRAPPER_RESOURCE_CHANNEL_FD
    );
    assert_ne!(
        relocated.as_raw_fd(),
        underwrap_protocol::exec::WRAPPER_PARENT_LIVENESS_FD
    );
}

fn wait(pid: libc::pid_t) -> libc::c_int {
    let mut status = 0;
    loop {
        // SAFETY: pid is the direct child and status is writable.
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result == pid {
            return status;
        }
        assert!(
            result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        );
    }
}

#[test]
fn sigchld_normalization_reaps_then_restores_inherited_state() {
    // SAFETY: all signal-state mutations happen in this isolated child.
    let harness = unsafe { libc::fork() };
    assert!(harness >= 0);
    if harness == 0 {
        let mut ignored: libc::sigaction = unsafe { std::mem::zeroed() };
        ignored.sa_sigaction = libc::SIG_IGN;
        ignored.sa_flags = libc::SA_NOCLDWAIT;
        unsafe { libc::sigemptyset(&mut ignored.sa_mask) };
        if unsafe { libc::sigaction(libc::SIGCHLD, &ignored, std::ptr::null_mut()) } != 0
            || set_signal_blocked(libc::SIGCHLD, true).is_err()
        {
            unsafe { libc::_exit(10) }
        }
        let saved = SigchldState::capture().unwrap();
        saved.normalize_for_wait().unwrap();
        let child = unsafe { libc::fork() };
        if child == 0 {
            unsafe { libc::_exit(0) }
        }
        if child < 0 || !libc::WIFEXITED(wait(child)) || saved.restore().is_err() {
            unsafe { libc::_exit(11) }
        }
        let mut restored: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut restored) };
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask) };
        let okay = restored.sa_sigaction == libc::SIG_IGN
            && restored.sa_flags & libc::SA_NOCLDWAIT != 0
            && unsafe { libc::sigismember(&mask, libc::SIGCHLD) } == 1;
        unsafe { libc::_exit(if okay { 0 } else { 12 }) }
    }
    let status = wait(harness);
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

#[test]
fn blocked_child_signal_is_reset_unblocked_and_reproduced() {
    let child = unsafe { libc::fork() };
    assert!(child >= 0);
    if child == 0 {
        set_signal_blocked(libc::SIGTERM, true).unwrap();
        propagate_wait_status(libc::SIGTERM);
    }
    let status = wait(child);
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGTERM);
}
