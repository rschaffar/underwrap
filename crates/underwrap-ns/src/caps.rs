pub(crate) fn set_no_new_privs() -> Result<(), String> {
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// Drop all Linux capabilities.
pub(crate) fn drop_caps() -> Result<(), String> {
    unsafe {
        let ret = libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        );
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EINVAL) {
                return Err(format!("prctl(PR_CAP_AMBIENT_CLEAR_ALL) failed: {err}"));
            }
        }

        for cap in 0..64 {
            let ret = libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(libc::EINVAL) => break,
                    Some(libc::EPERM) => break,
                    _ => return Err(format!("prctl(PR_CAPBSET_DROP, {cap}) failed: {err}")),
                }
            }
        }

        #[repr(C)]
        struct CapHeader {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct CapData {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = CapHeader {
            version: 0x20080522,
            pid: 0,
        };
        let data = [CapData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];

        let ret = libc::syscall(libc::SYS_capset, &header as *const CapHeader, data.as_ptr());
        if ret != 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("capset() failed: {err}"));
        }
    }
    Ok(())
}
