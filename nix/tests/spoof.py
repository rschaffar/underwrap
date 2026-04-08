#!/usr/bin/env python3
"""Attempt to spoof a wrapper request on a wrapper socket."""

import socket
import sys

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect("/run/underwrap-local/wrapper-test-echo.sock")
except PermissionError:
    print("Permission denied (expected)", file=sys.stderr)
    sys.exit(0)
except OSError as e:
    print(f"Connect failed as expected: {e}", file=sys.stderr)
    sys.exit(0)

print("SPOOFED: connected to wrapper socket unexpectedly", file=sys.stderr)
sys.exit(1)
