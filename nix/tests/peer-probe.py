#!/usr/bin/env python3
"""Same-host NON-ANCESTOR (peer) probe against an isolated target (story 2600 / 2660).

Runs as the caller's uid (`testuser`) but is **not** an ancestor of the target —
it is a separately spawned peer process, the realistic "another process I own
goes looking for the secret" threat. It takes the target PID as `argv[1]` and
attempts the two ATTACH-class syscalls the per-target uid boundary must deny:

  * `ptrace(PTRACE_ATTACH)`  — the debugger vector,
  * `process_vm_readv`       — direct cross-process memory read.

Both are gated by `ptrace_may_access`: the dedicated sandbox uid X differs from
the caller's uid, so the uid check fails (and `ptrace_scope=2` independently
requires `CAP_SYS_PTRACE`). The probe prints `RESULT:` lines parsed by the VM
test; it must never recover the injected secret.
"""
import ctypes
import sys

pid = int(sys.argv[1])
secret_markers = (b"iso_secret_val", b"ISO_SECRET")

libc = ctypes.CDLL("libc.so.6", use_errno=True)

# --- ptrace(PTRACE_ATTACH) -------------------------------------------------
PTRACE_ATTACH = 16
PTRACE_DETACH = 17
libc.ptrace.restype = ctypes.c_long
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
ctypes.set_errno(0)
ret = libc.ptrace(PTRACE_ATTACH, pid, None, None)
if ret == 0:
    # Should never happen for the isolated target; detach so we don't wedge it.
    libc.ptrace(PTRACE_DETACH, pid, None, None)
    print("RESULT:PTRACE_ALLOWED")
else:
    print(f"RESULT:PTRACE_DENIED(errno={ctypes.get_errno()})")


# --- process_vm_readv ------------------------------------------------------
class iovec(ctypes.Structure):
    _fields_ = [("iov_base", ctypes.c_void_p), ("iov_len", ctypes.c_size_t)]


buf = ctypes.create_string_buffer(256)
local = iovec(ctypes.cast(buf, ctypes.c_void_p), ctypes.c_size_t(len(buf)))
# Any plausible mapped address: the uid gate is checked before the address is
# ever dereferenced, so EPERM (not EFAULT) is the expected denial.
remote = iovec(ctypes.c_void_p(0x400000), ctypes.c_size_t(len(buf)))
libc.process_vm_readv.restype = ctypes.c_ssize_t
libc.process_vm_readv.argtypes = [
    ctypes.c_int,
    ctypes.POINTER(iovec),
    ctypes.c_ulong,
    ctypes.POINTER(iovec),
    ctypes.c_ulong,
    ctypes.c_ulong,
]
ctypes.set_errno(0)
n = libc.process_vm_readv(pid, ctypes.byref(local), 1, ctypes.byref(remote), 1, 0)
if n >= 0:
    data = buf.raw[:n]
    if any(m in data for m in secret_markers):
        print("RESULT:VM_READV_SECRET_LEAKED")
    else:
        print("RESULT:VM_READV_ALLOWED")
else:
    print(f"RESULT:VM_READV_DENIED(errno={ctypes.get_errno()})")
