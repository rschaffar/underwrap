use super::*;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

struct AdoptedChild {
    pid: i32,
    pidfd: OwnedFd,
    reaped: bool,
}

impl AdoptedChild {
    fn reap(&mut self, budget: Duration) -> std::io::Result<i32> {
        let deadline = Instant::now() + budget;
        loop {
            let mut status = 0;
            // SAFETY: the isolated subreaper adopts this exact grandchild;
            // status is writable and WNOHANG never blocks the external bound.
            let result = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if result == self.pid {
                self.reaped = true;
                return Ok(status);
            }
            if result < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for AdoptedChild {
    fn drop(&mut self) {
        if !self.reaped {
            // Independent of the group-cleanup mechanism under test. The pidfd
            // binds this fallback signal to the original identity, not PID reuse.
            // SAFETY: live pidfd, fixed signal, no siginfo pointer.
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.pidfd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            let _ = self.reap(Duration::from_secs(5));
        }
    }
}

#[test]
fn supervisor_cleans_group_even_after_witness_exits() {
    const NAME: &str = "test_support::tests::supervisor_cleans_group_even_after_witness_exits";
    const MODE: &str = "UNDERWRAP_SUPERVISOR_CONTROL";
    match std::env::var(MODE).ok().as_deref() {
        Some("grandchild") => loop {
            std::thread::park();
        },
        Some("witness") => {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([NAME, "--exact", "--test-threads=1"])
                .env(MODE, "grandchild")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let child = command.spawn().unwrap();
            // Deliberate control: this raw handle transfers cleanup responsibility
            // to the already-running external supervisor, to test its safety net.
            let pid_path =
                std::path::PathBuf::from(std::env::var_os("UNDERWRAP_CONTROL_PID").unwrap());
            let pending = pid_path.with_extension("pending");
            std::fs::write(&pending, child.id().to_string()).unwrap();
            std::fs::rename(pending, pid_path).unwrap();
            let ack = std::env::var_os("UNDERWRAP_CONTROL_ACK").unwrap();
            while !std::path::Path::new(&ack).exists() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::mem::forget(child);
        }
        Some("supervisor") => {
            // SAFETY: this is a fresh single-test process. Subreaper state is
            // confined here so the control can reap the deliberately orphaned
            // grandchild; the parallel Cargo test process is never changed.
            assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
            let state = tempfile::TempDir::new().unwrap();
            let pid_path = state.path().join("pid");
            let ack = state.path().join("ack");
            let mut worker = ExternalTest::spawn(
                NAME,
                &[
                    (MODE, "witness".as_ref()),
                    ("UNDERWRAP_CONTROL_PID", pid_path.as_os_str()),
                    ("UNDERWRAP_CONTROL_ACK", ack.as_os_str()),
                ],
            );
            while !pid_path.exists() {
                worker.check_running();
                std::thread::sleep(Duration::from_millis(5));
            }
            let pid: u32 = std::fs::read_to_string(pid_path).unwrap().parse().unwrap();
            // SAFETY: scalar pidfd_open creates one identity-bound descriptor.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            assert!(fd >= 0);
            let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
            let mut adopted = AdoptedChild {
                pid: pid as i32,
                pidfd: fd,
                reaped: false,
            };
            std::fs::write(ack, b"1").unwrap();
            assert!(worker.wait().success());
            // Actual bounded reap, not just a signal attempt; the independent
            // guard above still cleans up if group termination regresses.
            let status = adopted
                .reap(Duration::from_secs(2))
                .expect("supervisor did not kill its surviving child");
            assert!(libc::WIFSIGNALED(status));
            assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
            let mut pollfd = libc::pollfd {
                fd: adopted.pidfd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one writable pollfd with a live pidfd.
            assert_eq!(unsafe { libc::poll(&mut pollfd, 1, 0) }, 1);
            assert_ne!(pollfd.revents & libc::POLLIN, 0);
        }
        None => {
            let mut worker = ExternalTest::spawn(NAME, &[(MODE, "supervisor".as_ref())]);
            // Leave time for the nested supervisor's 30-second failure and the
            // independent adopted-child guard before the outer hard bound.
            worker.deadline = Instant::now() + Duration::from_secs(60);
            assert!(
                worker.wait().success(),
                "isolated supervisor safety-net control failed"
            );
        }
        _ => panic!("invalid internal supervisor control selector"),
    }
}
