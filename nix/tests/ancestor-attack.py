#!/usr/bin/env python3
"""Adversarial ancestor attack against the target-UID boundary.

This script runs as a PARENT (ancestor) process that spawns wrapper
`test-iso`. The wrapped target and the descendant it backgrounds (`sleep 5252`)
run under a dedicated sandbox uid X, distinct from this caller. The dedicated
UID is what makes `ptrace_may_access` fail the UID gate for BOTH the READ-mode
files (`/proc/<pid>/environ`) and the ATTACH-mode vectors (`ptrace`,
`/proc/<pid>/mem`), so even an ancestor cannot recover the secret.

The unique `sleep 5252` marker means the /proc scan locates exactly this run's
target, never a stale process from an earlier scenario.

Output lines prefixed with RESULT: are parsed by the VM test.
"""
import subprocess, time, os, sys, ctypes

# Spawn the wrapper as our child. isoScript backgrounds `sleep 5252` (the
# descendant) and waits; the whole subtree runs as the dedicated uid X.
wrapper = subprocess.Popen(
    ["/run/wrappers/bin/underwrap-local", "test-iso"],
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
time.sleep(2)

# Find the descendant by its unique marker. It runs as uid X (NOT our
# uid), so we deliberately do NOT filter on our own uid — we want to prove that
# even though we can *see* the process exists, we cannot read its secret state.
target_pid = None
for pid_dir in os.listdir("/proc"):
    if not pid_dir.isdigit():
        continue
    try:
        cmdline = open(f"/proc/{pid_dir}/cmdline", "rb").read()
        if b"sleep\x005252" in cmdline:
            target_pid = pid_dir
            break
    except (PermissionError, FileNotFoundError, ProcessLookupError):
        continue

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
    if "ISO_SECRET" in env_str or "iso_secret_val" in env_str:
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
