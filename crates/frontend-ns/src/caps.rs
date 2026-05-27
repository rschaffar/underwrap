#[cfg(test)]
mod tests;

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
            // EINVAL is the only acceptable failure: older kernels lack
            // PR_CAP_AMBIENT entirely, in which case there is no ambient set
            // to clear and the operation is a no-op. Any other errno is real.
            if err.raw_os_error() != Some(libc::EINVAL) {
                return Err(format!("prctl(PR_CAP_AMBIENT_CLEAR_ALL) failed: {err}"));
            }
        }

        for cap in 0..64 {
            let ret = libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    // EINVAL terminates the loop: the kernel reports it once
                    // `cap` exceeds the highest capability it knows about.
                    Some(libc::EINVAL) => break,
                    // EPERM means we lack CAP_SETPCAP in the effective set, so
                    // the bounding set cannot be shrunk here. This is the
                    // normal case under NixOS `security.wrappers`: the wrapper
                    // deliberately strips CAP_SETPCAP from the ambient set it
                    // grants the wrapped binary (see nixpkgs setcap-wrapper.c),
                    // so underwrap-ns never receives it even when the module
                    // lists `cap_setpcap+ep`. The bounding set is not the
                    // security boundary anyway: the `capset` below clears the
                    // permitted/effective/inheritable sets to empty, and
                    // PR_SET_NO_NEW_PRIVS prevents regaining caps across exec,
                    // so a populated bounding set grants nothing.
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
