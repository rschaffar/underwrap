# Testing

## Cargo tests (85+)

Run with `cargo test --workspace`. Covers:

| Crate | Tests | What |
|-------|-------|------|
| `underwrap-protocol` | 20 | Message serialization, config parsing, IPC framing, backend protocol |
| `underwrap-store` | 19 | Plain + encrypted backends, path traversal rejection, atomic writes, wrong-key detection |
| `underwrapd` | 17 | Unlock state (time expiry, scopes, UID binding, admin grants), OAuth derivation, token cache |
| `integration-tests` | 29 | Full daemon lifecycle, store/read flow, immutability, concurrent access, binary-level exec tests |

Binary-level integration tests (exec wrapper, underwrapctl) skip gracefully
when `underwrap-ns` with `cap_sys_admin` is not available (outside the NixOS VM).

## NixOS VM test (21 assertions)

Run with `nix build .#checks.x86_64-linux.vm-test`.

A single VM boots with both a local-storage instance and a remote-backend
instance, plus the backend daemon and SSH server. An unprivileged user
(`testuser`, UID 1099) exercises the full wrapper chain.

### Test structure

The VM configuration is in `nix/tests/node.nix`. The test script is
`nix/tests/test.py`. A spoofing helper is `nix/tests/spoof.py`.

### Local instance assertions

| # | Assertion | Security property |
|---|-----------|-------------------|
| 1 | Socket permissions correct | 775 dir, 666 read, 600 control |
| 2 | Setgid wrapper installed with correct group | Wrapper identity mechanism works |
| 3 | `underwrap-ns` has `cap_sys_admin` | PID namespace creation possible |
| 4 | Unprivileged user runs wrapper, gets secret | End-to-end setgid flow |
| 5 | Second wrapper works | Not a single-wrapper fluke |
| 6 | Daemon resolved identity from GID | Kernel-verified, not client-supplied |
| 7 | Control socket blocked for normal user | Can't store/unlock directly |
| 8 | Secret files blocked for normal user | Filesystem permissions hold |
| 9 | Wrapper output contains only its own secret | Cross-wrapper isolation |
| 10 | Raw socket spoofing rejected | Python script sends forged request, daemon rejects |
| 11 | Locked wrapper blocks unprivileged user | Unlock required |
| 12 | Lock/unlock lifecycle | Unlock → use → lock → denied |
| 13 | Secrets persist across daemon restart | Storage durability |
| 14 | Target `/proc/<host_pid>/environ` not readable by same-UID | Yama LSM (ptrace_scope=1) blocks non-descendant access |
| 15 | Target `/proc/<host_pid>/mem` not readable by same-UID | Same Yama mechanism |
| 16 | Wrapper `/proc/pid/environ` not readable by same-UID | `PR_SET_DUMPABLE=0` persists (wrapper never execs) |
| 17 | No core dump on SIGSEGV | `RLIMIT_CORE=0` works |

### Remote instance assertions

| # | Assertion | Security property |
|---|-----------|-------------------|
| 18 | Wrapper fails without tunnel | No backend socket = no secrets |
| 19 | SSH tunnel established, wrapper succeeds | Full tunnel flow works |
| 20 | No secret on local disk | Truly remote storage |
| 21 | Wrapper fails after tunnel killed | Disconnect = lock |
| + | Wrapper works after tunnel reconnect | Reconnect = unlock |

### Cross-cutting assertions

| Assertion | What it proves |
|-----------|----------------|
| Local wrapper doesn't leak remote secrets | Instance isolation |
| Audit events present in journald | Structured logging works |
| No secret values in journald | Material never logged |
