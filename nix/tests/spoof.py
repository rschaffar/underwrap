#!/usr/bin/env python3
"""Attempt to spoof a wrapper request on the raw read socket."""

import socket, json, struct, sys

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect("/run/underwrap-local/read.sock")
payload = json.dumps({"action": "read", "wrapper": "test-echo", "scope": None}).encode()
s.sendall(struct.pack(">I", len(payload)) + payload)

# Read length-prefixed response.
header = b""
while len(header) < 4:
    chunk = s.recv(4 - len(header))
    if not chunk:
        print("Connection closed (rejected)", file=sys.stderr)
        sys.exit(0)
    header += chunk

length = struct.unpack(">I", header)[0]
body = b""
while len(body) < length:
    chunk = s.recv(length - len(body))
    if not chunk:
        break
    body += chunk

resp = json.loads(body)
if resp.get("status") == "ok":
    print("SPOOFED: got secret!", file=sys.stderr)
    sys.exit(1)
print("Rejected:", resp.get("message", ""), file=sys.stderr)
sys.exit(0)
