# underwrap — TODO

## Open issues

### Unlock for non-root users (partially resolved)

Root unlock grants now create admin grants (uid=None) that cover all UIDs.
A normal user can use a locked wrapper after root runs `underwrapctl unlock`.

Remaining: normal users still cannot *initiate* an unlock because the control
socket is 0600. Options for future:
- Setgid `underwrapctl` wrapper for unlock operations
- Make unlock a command on the read socket (authenticated by wrapper GID)

### Immutability TOCTOU

Two concurrent stores to a new immutable secret can both observe "absent" and
both succeed. Low practical risk (stores are rare admin operations on the
restricted control socket) but violates the documented contract.

Fix: per-secret mutex in the store path.

### NixOS module: injection config duplication

`injection.variable` (daemon config) and `envVariable` (sidecar config) can
drift. Should derive one from the other.

### Status error suppression

`build_status()` catches storage read errors and reports them as "not stored",
which can hide corruption.

### PR_SET_DUMPABLE requires suid_dumpable=0

The `pre_exec` hook sets `PR_SET_DUMPABLE=0`, but the kernel resets it after
exec of a non-setuid target when `suid_dumpable != 0`. NixOS VMs default to
`suid_dumpable=2`. For full `/proc/pid/environ` protection, the system needs
`boot.kernel.sysctl."fs.suid_dumpable" = 0` or the NixOS module should set it.

### Encrypted backend: key management is weak

The encrypted storage backend reads its master key from a plaintext file on
the same disk as the encrypted secrets. This provides minimal real protection —
if an attacker can read the secret files, they can almost certainly read the
key file too.

To make this meaningful, the master key should be derived from a passphrase
at daemon startup (e.g. Argon2 KDF), so the key never exists on disk at rest.
The daemon would prompt for the passphrase on start (or receive it via a
socket/systemd credential), derive the key in memory, and zeroize the
passphrase. This ties naturally into the unlock model — the daemon starts
locked and can't serve secrets until the passphrase is provided.

Other future options: TPM-sealed key, YubiKey challenge-response, or
integration with a remote KMS. None of these are planned now.

## Not yet implemented

- [ ] stdin and tempfile injection modes in `underwrap-exec`
- [ ] Secret Service / libsecret-compatible delivery mode for wrapped tools that support it
  - Provide a minimal, wrapper-scoped Secret Service API to the child process
  - Prefer a private D-Bus session/bus with access limited to only the secret(s) needed by that wrapper
  - Keep `env` injection as a supported fallback for tools that do not support libsecret / Secret Service
- [ ] `underwrapctl reauth` workflow for OAuth credential refresh
- [ ] SSH-agent challenge auth for unlock
- [ ] Secret metadata / versioning
- [ ] Home Manager helpers (scope setup, completions)
- [ ] Generic Linux packaging (deb, rpm)
- [ ] Startup warning when socket auth allowlists are empty

## Deferred scope

The following are explicitly not planned for now:

- **External secret providers**: Secret Service / libsecret, Vault, AWS SSM,
  1Password, Doppler — none built-in. When needed, likely via exec-based
  providers behind the `StorageBackend` trait.
- **Provider plugin SPI**: no exec-based or shared-library plugin system yet.
  The trait is the extension point.
- **Hardware-assisted unseal**: TPM, YubiKey for the encrypted backend's
  master key.
- **sops / age import pipeline**
