# Task: Remove the invite/enrollment flow; replace with manual paste-cert admin UX

Parent: greenfield-cleanup (no parent story; this ticket documents a
ripout that was decided directly on the first-release polish path).

## Context

The earlier `epic_dynamic_runtime_clients_enrollment_and_stores.md`
introduced an operator-approved enrollment invite flow:

- An admin created an "enrollment invite" via the SPA, the backend
  handed back a one-time secret + helper command.
- The operator pasted that into a shell on the target host where the
  `underwrap-enroll` helper prompted for the secret, generated a
  self-signed cert, and posted it to `POST /v1/enroll` together with
  the invite id + secret.
- The backend stored the cert's SHA-256 fingerprint as the client's
  current credential.

For greenfield, this turned out to be a lot of moving parts for the
amount of safety it bought. We already had `POST /v1/admin/client-instances`
for direct admin-driven creation; the invite flow was effectively a
second creation path with its own protocol type set, its own backend
state file, its own SPA page, and its own integration-test suite.

## Outcome

Ripped out for the first release. The new flow is:

- `underwrap-keygen` runs on the target host, generates a keypair +
  self-signed cert locally, writes the credential files atomically
  with the right ownership, and prints the cert PEM to stdout. It
  does not talk to the backend.
- An admin pastes that PEM into the **Create client instance** form on
  the **Client Instances** page (or into the new **Edit cert** form to
  rotate). The existing admin API for client-instance CRUD now owns
  the entire registration surface, with a new optional `cert_pem`
  field on `AdminClientInstanceUpdateRequest` for credential rotation.

## What changed

- `crates/frontend-enroll/` renamed to `crates/frontend-keygen/`,
  binary renamed from `underwrap-enroll` to `underwrap-keygen`.
- Stripped all network code from the helper: `reqwest`, `rustls`,
  `rustls-native-certs`, `sha2`, the TTY echo-disable prompt, the
  `--url` / `--invite-id` / `--accept-server-ca-fingerprint` flags,
  and the fingerprint-pinning / accept-any TLS verifiers.
- Removed `crates/backend-backendd/src/enrollments.rs` and every
  enrollment-related handler/route/state/test on the backend.
- Removed `EnrollmentSubmitRequest`/`Response`,
  `AdminEnrollmentInvite*` and the `/v1/enroll` and
  `/v1/admin/enrollment-invites` routes.
- Deleted the SPA's `/enroll` page + tests and the
  `postEnrollmentInviteCreate` API client.
- Added a **Create client instance** dialog (with a `cert_pem` textarea)
  and an **Edit cert** dialog to the client-instances page, plus the
  new `postClientInstanceCreate` API client.
- Added an optional `cert_pem` field to
  `AdminClientInstanceUpdateRequest`; the backend's update handler
  parses it and replaces `current_credential`.
- Updated `docs/{backend,security,remote-backend,getting-started}.md`.

## Status

Done.
