# underwrap — TODO

## Resolved

### /proc/pid/environ protection
Two-stage wrapper: underwrap-exec (setgid) → underwrap-ns (cap_sys_admin).
Target runs in PID namespace, invisible to parent namespace.

### Wrapper process memory protection
Both wrapper parent processes call `prctl(PR_SET_DUMPABLE, 0)` and zeroize
the secret after handoff. Since they never exec again, the flag persists.

### Core dump prevention
`RLIMIT_CORE=0` set in PID namespace child before exec.

### Unlock for non-root users
Root unlock grants create admin grants (uid=None) covering all UIDs.

### Immutability TOCTOU
Per-secret write serialization prevents concurrent stores from both
observing "absent" and bypassing the immutability check.

### NixOS module injection config duplication
`envVariable` removed. Sidecar config derives `env_variable` from
`injection.variable` — single source of truth.

### Status error suppression
`build_status()` now surfaces storage read errors as `storage_error`
field instead of silently reporting "not stored".

## Not yet implemented

- [ ] stdin and tempfile injection modes in `underwrap-exec`
- [ ] `underwrapctl reauth` workflow for OAuth credential refresh
- [ ] SSH-agent challenge auth for unlock
- [ ] Self-service unlock for non-root users (root can unlock for them today)
- [ ] Secret metadata / versioning
- [ ] Home Manager helpers (scope setup, completions)
- [ ] Generic Linux packaging (deb, rpm)

## Deferred scope

- **External secret providers**: via exec-based providers behind `StorageBackend` trait
- **Hardware-assisted unseal**: TPM, YubiKey for encrypted backend master key
- **Encrypted backend key management**: passphrase-derived key via Argon2
- **sops / age import pipeline**
