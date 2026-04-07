# Testing

## Cargo tests (126+)

Run with `cargo test --workspace`.

Includes:
- Protocol serialization roundtrips
- Storage backend CRUD (plain, encrypted) + list/delete
- Daemon read/write/unlock/lock lifecycle
- Secretsd channel scoping and traversal rejection
- Secret Service store operations (collections, items, search)
- unlock_key SSH challenge-response on same connection
- SCM_RIGHTS fd passing roundtrip

Run with `cargo test --workspace`. Covers:

| Crate | Tests | What |
|-------|-------|------|
| `underwrap-protocol` | 20 | Message serialization, config parsing, IPC framing, backend protocol |
| `underwrap-store` | 19 | Plain + encrypted backends, path traversal rejection, atomic writes, wrong-key detection |
| `underwrapd` | 17 | Unlock state (time expiry, scopes, UID binding, admin grants), OAuth derivation, token cache |
| `integration-tests` | 29 | Full daemon lifecycle, store/read flow, immutability, concurrent access, binary-level exec tests |

Binary-level integration tests (exec wrapper, underwrapctl) skip gracefully
when `underwrap-ns` with `cap_sys_admin` is not available (outside the NixOS VM).

## NixOS VM test (26+ assertions)

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
| 18 | Target capabilities (CapEff/CapPrm/CapInh/CapAmb) are zero | `capset()` clears all capability sets |
| 19 | `secret-tool store` via libsecret wrapper | D-Bus Secret Service works end-to-end |
| 20 | `secret-tool lookup` retrieves stored secret | Persistence through backend |
| 21 | Private bus socket not visible from host | Namespace-private tmpfs isolation |
| 22 | No secret material leaked outside backend dir | Storage goes through StorageBackend only |

### Remote instance assertions

| # | Assertion | Security property |
|---|-----------|-------------------|
| 23 | Wrapper fails without tunnel | No backend socket = no secrets |
| 24 | SSH tunnel established, wrapper succeeds | Full tunnel flow works |
| 25 | No secret on local disk | Truly remote storage |
| 26 | Wrapper fails after tunnel killed | Disconnect = lock |
| + | Wrapper works after tunnel reconnect | Reconnect = unlock |

### Cross-cutting assertions

| Assertion | What it proves |
|-----------|----------------|
| Local wrapper doesn't leak remote secrets | Instance isolation |
| Audit events present in journald | Structured logging works |
| No secret values in journald | Material never logged |
| Libsecret secret not in daemon logs | Channel ops don't leak material |
