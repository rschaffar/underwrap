# underwrap — TODO

## Current scope

Plain file-based local storage only. The daemon owns a directory of secret
files, reads and writes them directly. An encrypted variant (ChaCha20-Poly1305
with a master key file) is also available as a built-in backend.

Both backends implement the `StorageBackend` trait in the `underwrap-store` crate.

## What is explicitly deferred

The following are **not** in the immediate implementation plan:

- **Secret Service / libsecret integration** — no built-in optional backend
- **HashiCorp Vault** — no built-in backend
- **AWS SSM / Secrets Manager** — no built-in backend
- **1Password CLI** — no built-in backend
- **Doppler** — no built-in backend
- **sops / age import** — no built-in import pipeline
- **Provider / plugin SPI** — no exec-based or shared-library plugin system yet
- **Hardware-assisted unseal** (TPM, YubiKey) — not implemented

## Architecture direction for later

When external sources are needed, the likely approach is:

- A **pluggable provider layer** behind the existing `StorageBackend` trait
- Providers implemented as **external exec-based commands**: the daemon spawns
  a configured binary, passes the secret ID on argv or stdin, reads material
  from stdout. This avoids linking against provider-specific libraries.
- **Built-in backends stay minimal**: plain files and encrypted files are the
  only in-process implementations. Everything else goes through the exec
  provider interface.

None of this is being built now. The extension point is the trait; the design
is ready for it when the need arises.

## Near-term work

- [ ] Per-wrapper sockets for stronger wrapper identity isolation
- [ ] SSH-agent challenge auth for unlock
- [ ] stdin and tempfile injection modes in underwrap-exec
- [ ] `underwrapctl reauth` workflow for OAuth credential refresh
- [ ] Secret metadata / versioning
- [ ] Home Manager helpers (scope setup, completions)
- [ ] Generic Linux packaging (deb, rpm)
- [ ] Startup warning when socket auth allowlists are empty
