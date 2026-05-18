# Task: Replace client-cert CA with fingerprint pinning

## Motivation

Runtime-client authentication is ultimately decided by the SHA-256 fingerprint
match in `resolve_runtime_client` (`crates/backend-backendd/src/server.rs:706`).
An unknown-but-CA-signed cert still fails there; a known-fingerprint cert
passes regardless of chain. The whole enrollment CA — issuer keypair on the
backend, `client_ca_file` trust bundle, CSR-scrubbing logic — is therefore
structurally redundant for authentication. The chain only buys us free
notBefore/notAfter enforcement and parse sanity from
`WebPkiClientVerifier`, both cheaply replicated in a custom verifier.

This is a greenfield simplification, not a bug fix. It removes a signing key
the backend has to protect, drops a layer of NixOS deployment plumbing, and
collapses two trust statements (CA validity + fingerprint match) into one
(fingerprint match).

## Scope

Switch runtime-client mTLS to pure fingerprint pinning.

- Remove the backend enrollment CA:
  - `BackendConfig::issuer_cert_file` / `issuer_key_file`
  - `BackendConfig::client_ca_file`
  - `EnrollmentRuntimeState::issuer_cert_pem` / `issuer_key_pem` and their
    startup loading
  - `issue_client_certificate_from_csr` and its rcgen/CSR scrubbing
  - The `ca_cert_pem` field on the enrollment-submit response
  - The `load_cert_chain` + `WebPkiClientVerifier` wiring at
    `crates/backend-backendd/src/server.rs:589`
- Add a custom `rustls::server::danger::ClientCertVerifier` that:
  - Accepts any leaf the peer presents (no chain check)
  - Computes its SHA-256 fingerprint and either returns
    `CertificateError::UnknownIssuer` if no `Active` `ClientInstanceRecord`
    matches, or returns `Ok` and lets `resolve_runtime_client` do the
    authoritative lookup downstream
  - Enforces notBefore/notAfter on the leaf (or document why we skip it —
    the registry membership is the trust statement)
- Change the enrollment submit flow:
  - Helper generates a self-signed leaf locally instead of a CSR
  - Backend stores the leaf's fingerprint on the `ClientInstanceRecord`
    without re-issuing anything
  - The enrollment-submit response no longer returns a signed PEM; the
    helper's self-signed cert is the credential
- Update the `underwrap-enroll` CLI accordingly:
  - Drop `--ca-file` for client-cert chain purposes (server-cert trust is
    separate — see "Out of scope")
  - Generate a self-signed cert + key, write both to the configured paths
- Update the NixOS modules to stop deploying the issuer keypair / client CA
- Update `docs/security.md` and `docs/backend.md` to reflect the new model
- Update VM and integration tests; remove any test fixtures that assume a
  CA-signed runtime-client cert
- Update the admin SPA's `buildEnrollmentCommand`
  (`web/src/pages/enroll.tsx:66`) if the helper's flags change

## Out of scope

- The backend's **server** cert (verified by the daemon's mTLS client and the
  operator's browser). That's a separate trust anchor — symmetric server-side
  fingerprint pinning is a future discussion.
- Existing-deployment migration: re-enrollment is required and that's fine
  under the greenfield posture in `CLAUDE.md`.

## Acceptance criteria

- [ ] No `client_ca_file`, `issuer_cert_file`, or `issuer_key_file` remains in
      backend config, NixOS modules, or example configs.
- [ ] No CA signing private key is loaded or held by the backend at runtime.
- [ ] `cargo test --workspace` and `nix build .#checks.x86_64-linux.vm-test`
      pass with the new flow.
- [ ] A runtime client whose self-signed cert fingerprint is in
      `client_instances` (status `Active`) can connect to the backend and
      reach the correct store; a cert with an unknown fingerprint cannot,
      even if it would have chained to a CA under the old model.
- [ ] Re-enrollment replaces the stored fingerprint (interaction with
      `350_story_single_credential_reenrollment_and_immediate_effect.md`
      preserved).
- [ ] `docs/security.md` reflects fingerprint pinning as the authentication
      mechanism and no longer claims a CA-based trust model for runtime
      clients.

## Notes for the implementer

- The new `ClientCertVerifier` must implement
  `requires_raw_public_keys() -> false` and a `supported_verify_schemes()` that
  matches what we accept today. The `verify_tls12_signature` /
  `verify_tls13_signature` impls can delegate to `rustls::crypto::*` defaults.
- Be careful with `resolve_runtime_client`: today it relies on rustls having
  already validated the cert. After this change, the leaf is attacker-controlled
  bytes until the fingerprint check, so do not parse fields out of the cert for
  any purpose other than computing its hash.
- The CSR-scrubbing security invariant at `server.rs:3950` goes away with the
  CSR path, but its underlying lesson — "do not trust subject/SAN/EKU from a
  client-supplied cert" — must remain reflected in the new verifier: identity
  is the fingerprint, nothing else from the cert is consulted.
