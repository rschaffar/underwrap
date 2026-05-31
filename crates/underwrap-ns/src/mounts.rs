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

/// Stage the files-slot tmpfs root. Only invoked when the wrapper projects a
/// files slot; the mount-tree-private step is now a separate, unconditional
/// concern (see `make_mount_namespace_private`).
pub(crate) fn setup_directory_staging_tmpfs(staging_root_path: &str) -> Result<(), String> {
    std::fs::create_dir_all(staging_root_path)
        .map_err(|e| format!("creating directory staging mount point {staging_root_path}: {e}"))?;
    std::fs::set_permissions(staging_root_path, std::fs::Permissions::from_mode(0o000))
        .map_err(|e| format!("setting mountpoint permissions on {staging_root_path}: {e}"))?;
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

pub(crate) fn setup_mount_namespace() -> Result<(), String> {
    let bus_dir = "/tmp/.underwrap-ns-bus";
    std::fs::create_dir_all(bus_dir)
        .map_err(|e| format!("creating bus mount point {bus_dir}: {e}"))?;
    std::fs::set_permissions(bus_dir, std::fs::Permissions::from_mode(0o000))
        .map_err(|e| format!("setting mountpoint permissions on {bus_dir}: {e}"))?;
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
    std::fs::remove_dir(staging_root_path)
        .map_err(|e| format!("removing directory staging mount point: {e}"))
}
