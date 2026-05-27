use super::*;

/// `drop_caps` must succeed and leave an empty effective capability set even
/// when the process lacks `CAP_SETPCAP` (the normal case under NixOS
/// `security.wrappers`, which strips `CAP_SETPCAP` from the ambient set it
/// grants the wrapped binary). In that situation the `PR_CAPBSET_DROP` loop
/// cannot shrink the bounding set, but `capset` still clears the
/// permitted/effective/inheritable sets — which is the actual security
/// boundary. The check runs in a forked child so the destructive capability
/// changes stay out of the test runner.
#[test]
fn drop_caps_clears_effective_without_cap_setpcap() {
    const EXIT_OK_AND_EMPTY: i32 = 0;
    const EXIT_DROP_ERR: i32 = 2;
    const EXIT_STILL_HAS_CAPS: i32 = 3;
    const EXIT_SKIP: i32 = 4;

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

    unsafe {
        let pid = libc::fork();
        assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());

        if pid == 0 {
            let header = CapHeader {
                version: 0x2008_0522,
                pid: 0,
            };
            // Drop our own effective CAP_SETPCAP (and everything else) so the
            // bounding-set loop hits EPERM, exercising the path NixOS wrappers
            // actually run.
            let zero = [CapData {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            }; 2];
            if libc::syscall(libc::SYS_capset, &header as *const CapHeader, zero.as_ptr()) != 0 {
                libc::_exit(EXIT_SKIP);
            }

            if drop_caps().is_err() {
                libc::_exit(EXIT_DROP_ERR);
            }

            // Read back the effective set; it must be empty.
            let mut read_header = CapHeader {
                version: 0x2008_0522,
                pid: 0,
            };
            let mut read_data = [CapData {
                effective: 0xffff_ffff,
                permitted: 0xffff_ffff,
                inheritable: 0xffff_ffff,
            }; 2];
            if libc::syscall(
                libc::SYS_capget,
                &mut read_header as *mut CapHeader,
                read_data.as_mut_ptr(),
            ) != 0
            {
                libc::_exit(EXIT_SKIP);
            }
            if read_data[0].effective != 0 || read_data[1].effective != 0 {
                libc::_exit(EXIT_STILL_HAS_CAPS);
            }
            libc::_exit(EXIT_OK_AND_EMPTY);
        }

        let mut status: libc::c_int = 0;
        let waited = libc::waitpid(pid, &mut status, 0);
        assert_eq!(waited, pid, "waitpid failed");
        assert!(libc::WIFEXITED(status), "child did not exit normally");

        match libc::WEXITSTATUS(status) {
            EXIT_OK_AND_EMPTY => {}
            EXIT_SKIP => eprintln!("skipping: could not set/get capabilities in child"),
            EXIT_DROP_ERR => panic!("drop_caps returned Err without CAP_SETPCAP"),
            EXIT_STILL_HAS_CAPS => {
                panic!("drop_caps left a non-empty effective capability set")
            }
            other => panic!("child exited with unexpected code {other}"),
        }
    }
}
