use std::os::unix::fs::PermissionsExt;

/// Make the whole mount tree private+recursive so that any mount we perform
/// inside the namespace (staging tmpfs, fresh `/proc`, bus dir, real-bus hide)
/// does not propagate back to the host mount namespace. Called unconditionally
/// after `unshare(CLONE_NEWNS)` in every mode.
pub(crate) fn make_mount_namespace_private() -> Result<(), String> {
    let root = std::ffi::CString::new("/").unwrap();
    // SAFETY: `root` is a live NUL-terminated path; this propagation-only mount
    // operation permits null source, filesystem-type, and data pointers. Flags
    // are trusted and the return is checked inside the new mount namespace.
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
    // SAFETY: both CStrings and the static NUL-terminated options string remain
    // live for the call; trusted tmpfs arguments are checked before staging use.
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
    // SAFETY: both CStrings are live NUL-terminated buffers; null data is valid
    // for this trusted proc mount and the return is checked before target exec.
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
/// owned by the current fsuid. Under mandatory isolation this runs after the
/// child switches to uid X, so dbus-daemon / secretsd can create the socket and
/// write state there.
pub(crate) fn setup_mount_namespace() -> Result<(), String> {
    let bus_dir = "/tmp/.underwrap-ns-bus";
    std::fs::create_dir_all(bus_dir)
        .map_err(|e| format!("creating bus mount point {bus_dir}: {e}"))?;
    guard_mountpoint_mode(bus_dir)?;
    let bus_dir_c = std::ffi::CString::new(bus_dir).unwrap();
    let tmpfs_c = std::ffi::CString::new("tmpfs").unwrap();
    // SAFETY: both CStrings and the static NUL-terminated options string remain
    // live for the call; trusted tmpfs arguments are checked before helper use.
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

    Ok(())
}
