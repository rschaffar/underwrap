use std::os::unix::fs::PermissionsExt;

/// Make the whole mount tree private+recursive so that any mount we perform
/// inside the namespace (staging tmpfs, fresh `/proc`, bus dir, real-bus hide)
/// does not propagate back to the host mount namespace. Called unconditionally
/// after `unshare(CLONE_NEWNS)` in every mode.
pub(crate) fn make_mount_namespace_private() -> Result<(), String> {
    let root = std::ffi::CString::new("/").unwrap();
    let ret = unsafe {
        libc::mount(
            std::ptr::null(),
            root.as_ptr(),
            std::ptr::null(),
            libc::MS_PRIVATE | libc::MS_REC,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount(MS_PRIVATE|MS_REC) failed: {err}"));
    }
    Ok(())
}

/// Set a tmpfs mountpoint dir to `0000` as a pre-mount guard, **best-effort**.
///
/// The tmpfs mounted over this dir immediately shadows it, so the dir's own
/// permissions are moot once the mount succeeds; the guard only matters in the
/// brief single-threaded window before the mount, and only if the mount then
/// fails (in which case the dir is empty — no secret is ever placed in the
/// bare mountpoint). These mountpoints live on the shared `/tmp` at fixed
/// paths, so a dir created by a prior invocation under a **different uid** —
/// e.g. an isolated target's dedicated uid X created `/tmp/.underwrap-ns-*`
/// before a later caller-uid run — cannot be `chmod`'d by the current process.
/// That `EPERM` is expected and ignored: the tmpfs mount (with its own
/// `mode=` and the mounting fsuid as owner) covers it regardless. Any other
/// error is fatal.
fn guard_mountpoint_mode(path: &str) -> Result<(), String> {
    match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(e) => Err(format!("setting mountpoint permissions on {path}: {e}")),
    }
}

/// Stage the files-slot tmpfs root. Only invoked when the wrapper projects a
/// files slot; the mount-tree-private step is now a separate, unconditional
/// concern (see `make_mount_namespace_private`).
pub(crate) fn setup_directory_staging_tmpfs(staging_root_path: &str) -> Result<(), String> {
    std::fs::create_dir_all(staging_root_path)
        .map_err(|e| format!("creating directory staging mount point {staging_root_path}: {e}"))?;
    guard_mountpoint_mode(staging_root_path)?;
    let staging_root = std::ffi::CString::new(staging_root_path).unwrap();
    let tmpfs = std::ffi::CString::new("tmpfs").unwrap();
    let ret = unsafe {
        libc::mount(
            tmpfs.as_ptr(),
            staging_root.as_ptr(),
            tmpfs.as_ptr(),
            0,
            c"size=8m,mode=0700,noswap".as_ptr() as *const libc::c_void,
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINVAL) {
            return Err(
                "mount tmpfs for directory staging failed: kernel does not support tmpfs noswap"
                    .to_string(),
            );
        }
        return Err(format!("mount tmpfs for directory staging failed: {err}"));
    }
    Ok(())
}

/// Mount a fresh `proc` over `/proc` inside the new PID namespace. Must be
/// called from a process that is *inside* the new PID namespace (i.e. after
/// `fork_into_namespace`), otherwise the mount binds to the original PID
/// namespace and process discovery is not actually limited. Run in every mode
/// so the target only sees namespace-local PIDs.
pub(crate) fn remount_proc() -> Result<(), String> {
    let proc_path = std::ffi::CString::new("/proc").unwrap();
    let proc_type = std::ffi::CString::new("proc").unwrap();
    let ret = unsafe {
        libc::mount(
            proc_type.as_ptr(),
            proc_path.as_ptr(),
            proc_type.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount fresh /proc failed: {err}"));
    }
    Ok(())
}

/// Set up the private libsecret bus mount namespace: a tmpfs at the bus dir
/// (root owned by the current fsuid, so an isolated child running at uid X gets
/// an X-owned bus the dbus-daemon / secretsd it forks can write to) plus, for
/// an **unisolated** target, a bind of `/dev/null` over the caller's real
/// session bus so a target that ignores `DBUS_SESSION_BUS_ADDRESS` cannot reach
/// it. When `isolated`, the real-bus hide is skipped: the uid boundary already
/// denies a uid-X target access to the caller's `/run/user/<caller>` runtime
/// dir, and X cannot even traverse it to bind over the socket.
pub(crate) fn setup_mount_namespace(isolated: bool) -> Result<(), String> {
    let bus_dir = "/tmp/.underwrap-ns-bus";
    std::fs::create_dir_all(bus_dir)
        .map_err(|e| format!("creating bus mount point {bus_dir}: {e}"))?;
    guard_mountpoint_mode(bus_dir)?;
    let bus_dir_c = std::ffi::CString::new(bus_dir).unwrap();
    let tmpfs_c = std::ffi::CString::new("tmpfs").unwrap();
    let ret = unsafe {
        libc::mount(
            tmpfs_c.as_ptr(),
            bus_dir_c.as_ptr(),
            tmpfs_c.as_ptr(),
            0,
            c"size=4m,mode=0700".as_ptr() as *const libc::c_void,
        )
    };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("mount tmpfs for private bus failed: {err}"));
    }

    if isolated {
        return Ok(());
    }

    let uid = unsafe { libc::getuid() };
    let real_bus = format!("/run/user/{uid}/bus");
    if std::path::Path::new(&real_bus).exists() {
        let devnull = std::ffi::CString::new("/dev/null").unwrap();
        let target = std::ffi::CString::new(real_bus.as_str()).unwrap();
        let ret = unsafe {
            libc::mount(
                devnull.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            )
        };
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!(
                "hiding real session bus at {real_bus} failed: {err}"
            ));
        }
    }

    Ok(())
}

pub(crate) fn cleanup_directory_staging_root(staging_root_path: &str) -> Result<(), String> {
    let staging_root = std::ffi::CString::new(staging_root_path).unwrap();
    let ret = unsafe { libc::umount2(staging_root.as_ptr(), libc::MNT_DETACH) };
    if ret != 0 {
        return Err(format!(
            "unmounting directory staging root failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Remove the bare mountpoint, best-effort. The staging root is a fixed path
    // on the shared, sticky `/tmp`; if a prior isolated run created it under its
    // dedicated uid X, a later caller-uid run cannot remove that entry (sticky
    // bit) — that EPERM is expected and ignored. The empty dir is harmless and
    // is reused (mounted over) by the next run. Other errors are fatal.
    match std::fs::remove_dir(staging_root_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("removing directory staging mount point: {e}")),
    }
}
