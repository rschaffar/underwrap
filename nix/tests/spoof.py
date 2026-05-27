#!/usr/bin/env python3
"""Attempt to spoof a wrapper request on the single instance socket.

Run as an unprivileged user that holds neither the instance-group egid
(only the setgid `uw-<instance>` binary grants that transiently) nor
membership in the instance group. The socket is mode 0660 owned by
`underwrap-<instance>:underwrap-<instance>`, so the connect must be
refused at the kernel permission layer before the daemon reads a byte.
"""

import socket
import sys

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect("/run/underwrap-local/wrapper.sock")
except PermissionError:
    print("Permission denied (expected)", file=sys.stderr)
    sys.exit(0)
except OSError as e:
    print(f"Connect failed as expected: {e}", file=sys.stderr)
    sys.exit(0)

print("SPOOFED: connected to wrapper socket unexpectedly", file=sys.stderr)
sys.exit(1)
