#!/usr/bin/env python3
"""Adversarial ancestor attack test.

This script runs as a PARENT process that spawns the wrapper. The target
(sleep 300) becomes a descendant. Under Yama ptrace_scope=1, ancestor→
descendant tracing is allowed. This test determines whether the ancestor
can read the target's /proc/environ and recover the secret.

Output lines prefixed with RESULT: are parsed by the VM test.
"""
import subprocess, time, os, sys, ctypes

# Spawn the wrapper as our child.
wrapper = subprocess.Popen(
    ["/run/wrappers/bin/underwrap-local", "test-sleep", "300"],
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
time.sleep(2)

# Find the actual sleep process (our descendant).
target_pid = None
for pid_dir in os.listdir("/proc"):
    if not pid_dir.isdigit():
        continue
    try:
        cmdline = open(f"/proc/{pid_dir}/cmdline", "rb").read()
        status = open(f"/proc/{pid_dir}/status").read()
        if b"sleep\x00300" in cmdline:
            for line in status.splitlines():
                if line.startswith("Uid:"):
                    uid = int(line.split()[1])
                    if uid == os.getuid():
                        target_pid = pid_dir
                        break
    except (PermissionError, FileNotFoundError, ProcessLookupError):
        continue
    if target_pid:
        break

if not target_pid:
    print("RESULT:TARGET_NOT_FOUND")
    wrapper.terminate()
    wrapper.wait()
    sys.exit(0)

print(f"RESULT:TARGET_PID={target_pid}")

# Try to read the target's environ as its ancestor.
try:
    env_data = open(f"/proc/{target_pid}/environ", "rb").read()
    env_str = env_data.decode("utf-8", errors="replace")
    if "DUMPABLE_SECRET" in env_str or "dumpable_test_secret" in env_str:
        print("RESULT:SECRET_LEAKED")
    else:
        print("RESULT:ENVIRON_READABLE_NO_SECRET")
except PermissionError:
    print("RESULT:ENVIRON_DENIED")
except FileNotFoundError:
    print("RESULT:PROC_NOT_FOUND")

# Try ptrace attach.
libc = ctypes.CDLL("libc.so.6", use_errno=True)
PTRACE_ATTACH = 16
ret = libc.ptrace(PTRACE_ATTACH, int(target_pid), 0, 0)
if ret == 0:
    PTRACE_DETACH = 17
    libc.ptrace(PTRACE_DETACH, int(target_pid), 0, 0)
    print("RESULT:PTRACE_ALLOWED")
else:
    errno = ctypes.get_errno()
    print(f"RESULT:PTRACE_DENIED(errno={errno})")

# Try /proc/mem read.
try:
    with open(f"/proc/{target_pid}/mem", "rb") as f:
        f.read(1)
    print("RESULT:MEM_READABLE")
except PermissionError:
    print("RESULT:MEM_DENIED")
except (OSError, FileNotFoundError):
    print("RESULT:MEM_ERROR")

wrapper.terminate()
wrapper.wait()
