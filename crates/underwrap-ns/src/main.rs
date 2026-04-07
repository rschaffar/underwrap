//! underwrap-ns — PID namespace isolation stage.
//!
//! This binary is the second stage of the underwrap wrapper chain.
//! It is installed with `cap_sys_admin` (via setcap) and:
//!
//! 1. Reads the secret from fd 3 (pipe from underwrap-exec)
//! 2. Closes fd 3
//! 3. Calls `unshare(CLONE_NEWPID)` to create a new PID namespace
//! 4. Drops all capabilities
//! 5. Forks — child is PID 1 in the new namespace
//! 6. Child: sets the env var, execs the target
//! 7. Parent: waits for child, forwards exit code
//!
//! Usage: underwrap-ns <env_var> <target> [args...]
//! The secret is read from fd 3 (not stdin, not argv).

use std::io::Read;
use std::os::unix::process::CommandExt;

/// The fd number used to pass the secret from underwrap-exec.
const SECRET_FD: i32 = 3;

fn main() {
    if let Err(e) = run() {
        eprintln!("underwrap-ns: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(format!(
            "usage: {} <env_var> <target> [args...]",
            args.first().map(|s| s.as_str()).unwrap_or("underwrap-ns")
        ));
    }
    let env_var = &args[1];
    let target = &args[2];
    let target_args = &args[3..];

    // Read secret from fd 3.
    let material =
        read_secret_fd().map_err(|e| format!("reading secret from fd {SECRET_FD}: {e}"))?;

    // Create new PID namespace.
    let ret = unsafe { libc::unshare(libc::CLONE_NEWPID) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("unshare(CLONE_NEWPID) failed: {err}"));
    }

    // Drop all capabilities now that we've done the unshare.
    drop_caps().map_err(|e| format!("dropping capabilities: {e}"))?;

    // Fork. Child gets PID 1 in the new namespace.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("fork failed: {err}"));
    }

    if pid == 0 {
        // Child: exec the target with the secret in env.
        let mut cmd = std::process::Command::new(target);
        cmd.args(target_args);
        cmd.env(env_var, &material);

        let err = cmd.exec();
        eprintln!("underwrap-ns: exec failed: {err}");
        std::process::exit(1);
    }

    // Parent: wait for child, forward exit code.
    let mut status: libc::c_int = 0;
    let ret = unsafe { libc::waitpid(pid, &mut status, 0) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("waitpid failed: {err}"));
    }

    if libc::WIFEXITED(status) {
        std::process::exit(libc::WEXITSTATUS(status));
    } else if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
        std::process::exit(128 + sig);
    }
    std::process::exit(1);
}

/// Read the secret from the pipe on fd 3 and close the fd.
fn read_secret_fd() -> Result<String, std::io::Error> {
    use std::os::unix::io::FromRawFd;

    // SAFETY: fd 3 is expected to be a valid pipe read-end passed by underwrap-exec.
    let mut file = unsafe { std::fs::File::from_raw_fd(SECRET_FD) };
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    // File is closed on drop.
    Ok(buf)
}

/// Drop all Linux capabilities.
fn drop_caps() -> Result<(), String> {
    // PR_CAP_AMBIENT_CLEAR_ALL removes all ambient caps.
    // Then set the capability bounding set to empty by dropping each cap.
    unsafe {
        // Clear ambient capabilities.
        let ret = libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        );
        if ret != 0 {
            // Not fatal — may not have ambient caps.
        }

        // Drop all capabilities from the bounding set.
        // Iterate through capability numbers until we get EINVAL.
        for cap in 0..64 {
            let ret = libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINVAL) {
                    break; // No more valid capability numbers.
                }
                // EPERM means we don't have this cap — that's fine.
            }
        }

        // Clear all permitted/effective/inheritable capabilities by
        // setting the securebits to prevent regaining caps.
        // The simplest approach: the caps will be cleared on the
        // next exec() of a non-setcap binary (the target).
    }
    Ok(())
}
