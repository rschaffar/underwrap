use super::*;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

#[test]
fn inner_liveness_pipe_is_cloexec_and_directional() {
    let pipe = LivenessPipe::new().unwrap();
    for fd in [pipe.reader.0.as_raw_fd(), pipe.writer.0.as_raw_fd()] {
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
    assert_eq!(
        unsafe { libc::fcntl(pipe.reader.0.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE,
        libc::O_RDONLY
    );
    assert_eq!(
        unsafe { libc::fcntl(pipe.writer.0.as_raw_fd(), libc::F_GETFL) } & libc::O_ACCMODE,
        libc::O_WRONLY
    );
}

#[test]
fn arm_and_check_accepts_live_sole_writer_and_rejects_eof() {
    let pipe = LivenessPipe::new().unwrap();
    arm_and_check(&pipe.reader).unwrap();
    // SAFETY: clear the test process's temporary parent-death setting.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, 0) }, 0);
    let byte = [1_u8];
    // SAFETY: the byte is valid for this one-byte write to the owned pipe.
    assert_eq!(
        unsafe { libc::write(pipe.writer.0.as_raw_fd(), byte.as_ptr().cast(), 1) },
        1
    );
    assert!(arm_and_check(&pipe.reader).is_err());
    // SAFETY: clear the setting after the expected failure as well.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, 0) }, 0);
}

#[derive(Clone, Copy)]
enum DeathScenario {
    BeforeArm,
    AfterArm,
    AfterClearBeforeRearm,
}

fn kill_and_reap(pid: libc::pid_t) {
    // SAFETY: this test owns pid; SIGKILL makes the following reap bounded.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    loop {
        let mut status = 0;
        // SAFETY: status is writable and pid names the test-owned child.
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

fn wait_bounded(pid: libc::pid_t) -> Result<libc::c_int, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut status = 0;
        // SAFETY: status is writable and pid is a child adopted by the harness.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            return Ok(status);
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            kill_and_reap(pid);
            return Err(error.to_string());
        }
        if Instant::now() >= deadline {
            kill_and_reap(pid);
            return Err("timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn kill_group_and_drain_children(group: libc::pid_t) {
    // The middle process establishes a private process group before creating
    // the subject. Killing the group covers subjects whose PID was not reported
    // before an early harness failure.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
        libc::kill(group, libc::SIGKILL);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut status = 0;
        // SAFETY: the isolated subreaper owns every remaining child.
        let result = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if result > 0 {
            continue;
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                return;
            }
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if Instant::now() >= deadline {
            unsafe { libc::kill(-group, libc::SIGKILL) };
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_death_harness(scenario: DeathScenario) -> i32 {
    // Become a subreaper so the subject is reparented here when its immediate
    // parent dies; this lets the test always reap every process it creates.
    // SAFETY: prctl changes only this forked harness process.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } != 0 {
        return 10;
    }
    let mut report = [-1; 2];
    // SAFETY: report has two writable slots and is local to the harness.
    if unsafe { libc::pipe2(report.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return 11;
    }
    // SAFETY: the harness is single-threaded and both branches use only raw
    // pipe/process syscalls before _exit.
    let middle = unsafe { libc::fork() };
    if middle < 0 {
        return 12;
    }
    if middle == 0 {
        // SAFETY: isolate this branch and every later subject in one cleanup
        // group before creating further descendants.
        if unsafe { libc::setpgid(0, 0) } != 0 {
            unsafe { libc::_exit(9) };
        }
        unsafe { libc::close(report[0]) };
        let liveness = match LivenessPipe::new() {
            Ok(pipe) => pipe,
            Err(_) => unsafe { libc::_exit(13) },
        };
        let mut barrier = [-1; 2];
        let mut ready = [-1; 2];
        if unsafe { libc::pipe2(barrier.as_mut_ptr(), libc::O_CLOEXEC) } != 0
            || unsafe { libc::pipe2(ready.as_mut_ptr(), libc::O_CLOEXEC) } != 0
        {
            unsafe { libc::_exit(14) };
        }
        let subject = unsafe { libc::fork() };
        if subject < 0 {
            unsafe { libc::_exit(15) };
        }
        if subject == 0 {
            drop(liveness.writer);
            unsafe { libc::close(report[1]) };
            unsafe { libc::close(barrier[1]) };
            unsafe { libc::close(ready[0]) };
            let outcome = match scenario {
                DeathScenario::BeforeArm => {
                    let mut byte = 0_u8;
                    // EOF is the barrier proving the parent has exited.
                    unsafe { libc::read(barrier[0], (&mut byte as *mut u8).cast(), 1) };
                    arm_and_check(&liveness.reader).is_err()
                }
                DeathScenario::AfterArm => {
                    if arm_and_check(&liveness.reader).is_err() {
                        unsafe { libc::_exit(20) };
                    }
                    let byte = [1_u8];
                    unsafe { libc::write(ready[1], byte.as_ptr().cast(), 1) };
                    loop {
                        unsafe { libc::pause() };
                    }
                }
                DeathScenario::AfterClearBeforeRearm => {
                    if arm_and_check(&liveness.reader).is_err() {
                        unsafe { libc::_exit(21) };
                    }
                    // Simulate the credential transition clearing PDEATHSIG.
                    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, 0) } != 0 {
                        unsafe { libc::_exit(22) };
                    }
                    let byte = [1_u8];
                    unsafe { libc::write(ready[1], byte.as_ptr().cast(), 1) };
                    let mut byte = 0_u8;
                    unsafe { libc::read(barrier[0], (&mut byte as *mut u8).cast(), 1) };
                    arm_and_check(&liveness.reader).is_err()
                }
            };
            unsafe { libc::_exit(if outcome { 0 } else { 23 }) }
        }
        drop(liveness.reader);
        unsafe { libc::close(barrier[0]) };
        unsafe { libc::close(ready[1]) };
        // Report the subject before arranging immediate-parent death.
        let bytes = subject.to_ne_bytes();
        unsafe { libc::write(report[1], bytes.as_ptr().cast(), bytes.len()) };
        if matches!(
            scenario,
            DeathScenario::AfterArm | DeathScenario::AfterClearBeforeRearm
        ) {
            let mut byte = 0_u8;
            if unsafe { libc::read(ready[0], (&mut byte as *mut u8).cast(), 1) } != 1 {
                unsafe { libc::_exit(16) };
            }
        }
        // Closing both sole writers is the before-arm/rearm liveness proof;
        // exiting is the after-arm PDEATHSIG trigger.
        unsafe { libc::_exit(0) }
    }

    // Close the setpgid race from the parent side. EACCES means the child has
    // already performed its own successful transition/exec boundary.
    if unsafe { libc::setpgid(middle, middle) } != 0 {
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EACCES) | Some(libc::ESRCH)) {
            kill_group_and_drain_children(middle);
            return 29;
        }
    }
    unsafe { libc::close(report[1]) };
    let mut bytes = [0_u8; std::mem::size_of::<libc::pid_t>()];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = unsafe {
            libc::read(
                report[0],
                bytes[offset..].as_mut_ptr().cast(),
                bytes.len() - offset,
            )
        };
        if count < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if count <= 0 {
            kill_group_and_drain_children(middle);
            return 30;
        }
        offset += count as usize;
    }
    let subject = libc::pid_t::from_ne_bytes(bytes);
    let middle_status = match wait_bounded(middle) {
        Ok(status) => status,
        Err(_) => {
            kill_group_and_drain_children(middle);
            return 31;
        }
    };
    if !libc::WIFEXITED(middle_status) || libc::WEXITSTATUS(middle_status) != 0 {
        kill_group_and_drain_children(middle);
        return 32;
    }
    let subject_status = match wait_bounded(subject) {
        Ok(status) => status,
        Err(_) => {
            kill_group_and_drain_children(middle);
            return 33;
        }
    };
    match scenario {
        DeathScenario::AfterArm => {
            if libc::WIFSIGNALED(subject_status) && libc::WTERMSIG(subject_status) == libc::SIGKILL
            {
                0
            } else {
                34
            }
        }
        _ if (libc::WIFEXITED(subject_status) && libc::WEXITSTATUS(subject_status) == 0)
            || (libc::WIFSIGNALED(subject_status)
                && libc::WTERMSIG(subject_status) == libc::SIGKILL) =>
        {
            0
        }
        _ => 35,
    }
}

fn assert_death_scenario(scenario: DeathScenario) {
    // SAFETY: isolate signal/subreaper state in one child and always reap it.
    let harness = unsafe { libc::fork() };
    assert!(harness >= 0);
    if harness == 0 {
        let code = run_death_harness(scenario);
        unsafe { libc::_exit(code) }
    }
    let status = wait_bounded(harness).unwrap();
    assert!(libc::WIFEXITED(status), "harness status {status}");
    assert_eq!(libc::WEXITSTATUS(status), 0, "harness status {status}");
}

#[test]
fn outer_fixed_fd_admission_restores_cloexec_and_rejects_writer() {
    for use_writer in [false, true] {
        // SAFETY: descriptor collision/adoption is isolated in the child.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let pipe = LivenessPipe::new().unwrap();
            let source = if use_writer {
                pipe.writer.0.as_raw_fd()
            } else {
                pipe.reader.0.as_raw_fd()
            };
            // SAFETY: deliberate mapping to the fixed local ABI in the isolated
            // child; dup2 clears CLOEXEC as the real stage-1 exec does.
            if unsafe { libc::dup2(source, WRAPPER_PARENT_LIVENESS_FD) } < 0 {
                unsafe { libc::_exit(50) }
            }
            let result = adopt_outer_liveness();
            let okay = if use_writer {
                result.is_err()
            } else {
                result
                    .map(|reader| {
                        (unsafe { libc::fcntl(reader.raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC)
                            != 0
                    })
                    .unwrap_or(false)
            };
            unsafe { libc::_exit(if okay { 0 } else { 51 }) }
        }
        let status = wait_bounded(child).unwrap();
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }
}

#[test]
fn liveness_parent_death_before_arm_is_detected() {
    assert_death_scenario(DeathScenario::BeforeArm);
}

#[test]
fn liveness_parent_death_after_arm_is_sigkill() {
    assert_death_scenario(DeathScenario::AfterArm);
}

#[test]
fn liveness_clear_then_rearm_detects_dead_parent() {
    assert_death_scenario(DeathScenario::AfterClearBeforeRearm);
}

#[test]
fn sigchld_state_normalizes_wait_then_restores_target_state() {
    let harness = unsafe { libc::fork() };
    assert!(harness >= 0);
    if harness == 0 {
        // SAFETY: all signal mutations are isolated in this child process.
        let mut ignored: libc::sigaction = unsafe { std::mem::zeroed() };
        ignored.sa_sigaction = libc::SIG_IGN;
        ignored.sa_flags = libc::SA_NOCLDWAIT;
        unsafe { libc::sigemptyset(&mut ignored.sa_mask) };
        if unsafe { libc::sigaction(libc::SIGCHLD, &ignored, std::ptr::null_mut()) } != 0 {
            unsafe { libc::_exit(40) }
        }
        if set_signal_blocked(libc::SIGCHLD, true).is_err() {
            unsafe { libc::_exit(41) }
        }
        let saved = match SigchldState::capture() {
            Ok(saved) => saved,
            Err(_) => unsafe { libc::_exit(42) },
        };
        if saved.normalize_for_wait().is_err() {
            unsafe { libc::_exit(43) }
        }
        let child = unsafe { libc::fork() };
        if child == 0 {
            unsafe { libc::_exit(0) }
        }
        if child < 0 || wait_bounded(child).is_err() || saved.restore().is_err() {
            unsafe { libc::_exit(44) }
        }
        let mut restored: libc::sigaction = unsafe { std::mem::zeroed() };
        unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut restored) };
        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask) };
        let okay = restored.sa_sigaction == libc::SIG_IGN
            && restored.sa_flags & libc::SA_NOCLDWAIT != 0
            && unsafe { libc::sigismember(&mask, libc::SIGCHLD) } == 1;
        unsafe { libc::_exit(if okay { 0 } else { 45 }) }
    }
    let status = wait_bounded(harness).unwrap();
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}

#[test]
fn propagated_blocked_signal_reaches_direct_observer() {
    let child = unsafe { libc::fork() };
    assert!(child >= 0);
    if child == 0 {
        set_signal_blocked(libc::SIGTERM, true).unwrap();
        propagate_signal(libc::SIGTERM);
    }
    let status = wait_bounded(child).unwrap();
    assert!(libc::WIFSIGNALED(status));
    assert_eq!(libc::WTERMSIG(status), libc::SIGTERM);
}
