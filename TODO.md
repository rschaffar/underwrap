# underwrap — TODO

## Open issues

### Unlock for non-root users

The control socket is 0600 (underwrap user only), so normal users cannot run
`underwrapctl unlock`. Unlock grants are also bound to the caller's UID, so a
root-issued unlock doesn't satisfy a later read from UID 1000.

Options:
- Setgid `underwrapctl` wrapper for unlock operations
- Make unlock a command on the read socket (authenticated by wrapper GID)
- Dedicated unlock socket with broader group access

### Immutability TOCTOU

Two concurrent stores to a new immutable secret can both observe "absent" and
both succeed. Low practical risk (stores are rare admin operations on the
restricted control socket) but violates the documented contract.

Fix: per-secret mutex in the store path.

### VM test coverage for non-root users

The VM test runs everything as root. It does not verify that:
- A normal user can actually use the setgid wrappers
- Wrapper GID identity resolves correctly for non-root callers
- Unlock grants work across UID boundaries

### NixOS module: injection config duplication

`injection.variable` (daemon config) and `envVariable` (sidecar config) can
drift. Should derive one from the other.

### Status error suppression

`build_status()` catches storage read errors and reports them as "not stored",
which can hide corruption.

## Not yet implemented

- [ ] stdin and tempfile injection modes in `underwrap-exec`
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
