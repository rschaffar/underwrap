use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn all_projection_handles_are_close_on_exec() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    let child = root.create_child("child", 0o700).unwrap();
    child.create_file("file", b"synthetic").unwrap();
    let file = child.open_entry("file").unwrap();
    for fd in [root.0.as_raw_fd(), child.0.as_raw_fd(), file.as_raw_fd()] {
        // SAFETY: each fd is borrowed from a live owner; fcntl only reads flags.
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
    assert_eq!(
        std::fs::metadata(temp.path().join("child"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn staging_attachment_authority_requires_exact_owner_group_and_mode() {
    use std::os::unix::fs::MetadataExt;
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    let metadata = root.0.metadata().unwrap();
    let uid = metadata.uid();
    let gid = metadata.gid();
    assert!(root.require_identity_mode(uid, gid, 0o700).is_ok());
    assert!(
        root.require_identity_mode(uid.wrapping_add(1), gid, 0o700)
            .is_err()
    );
    assert!(
        root.require_identity_mode(uid, gid.wrapping_add(1), 0o700)
            .is_err()
    );
    assert!(root.require_identity_mode(uid, gid, 0o755).is_err());
    let attachment = root.target_child("attachment", 0).unwrap();
    assert!(attachment.require_identity_mode(uid, gid, 0).is_ok());
    std::fs::set_permissions(
        temp.path().join("attachment"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(attachment.require_identity_mode(uid, gid, 0).is_err());
}

#[test]
fn descriptor_traversal_rejects_invalid_components_and_symlinks() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    for name in ["", ".", "..", "a/b", "a\0b"] {
        assert!(root.create_child(name, 0o700).is_err());
        assert!(root.open_entry(name).is_err());
    }
    std::os::unix::fs::symlink(temp.path(), temp.path().join("link")).unwrap();
    assert!(root.open_child("link").is_err());
    assert!(root.open_entry("link").is_err());
    assert!(root.target_child("link", 0o700).is_err());
}

#[test]
fn target_mountpoint_handle_accepts_mode_zero_without_opening_contents() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_target_home(temp.path()).unwrap();
    let mountpoint = root.target_child("mountpoint", 0).unwrap();
    assert_eq!(
        mountpoint.0.metadata().unwrap().permissions().mode() & 0o777,
        0
    );
    assert!(root.target_child("mountpoint", 0).is_ok());
}

#[test]
fn directory_modes_ignore_restrictive_umask_in_a_disposable_child() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    // SAFETY: the child performs bounded fd/filesystem work and exits without
    // unwinding. Its umask change cannot affect this multithreaded test process.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        unsafe { libc::umask(0o777) };
        let result = (|| -> Result<(), String> {
            let child = root.create_child("private", 0o700)?;
            child.create_file("file", b"synthetic")?;
            let intermediate = root.target_child("target-parent", 0o700)?;
            intermediate.target_child("attachment", 0)?;
            Ok(())
        })();
        // Restore only exit instrumentation's umask, never operation's umask.
        unsafe {
            libc::umask(0o077);
            libc::_exit(if result.is_ok() { 0 } else { 1 })
        }
    }
    let mut status = 0;
    // SAFETY: wait for exactly our bounded child, retrying only EINTR.
    loop {
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid {
            break;
        }
        assert!(
            waited < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        );
    }
    assert!(libc::WIFEXITED(status));
    assert_eq!(libc::WEXITSTATUS(status), 0);
}
