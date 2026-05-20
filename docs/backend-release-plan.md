# Backend release plan

This is the consolidated plan for getting underwrap to a coherent first release
around the **central backend** architecture. It supersedes the three
`backend-v2-*.md` drafts, which will be removed once the plan is executed.

underwrap is greenfield (see `AGENTS.md`): the goal is the cleanest possible
first release, not backwards compatibility with transitional shapes that were
useful during development.

## Where we are

Shipped as of this plan:

- `underwrap-backendd` is an authenticated HTTPS service (mTLS client certs).
  Admin operations ride a sibling HTTP listener.
- `underwrapd` talks to the backend via `ServiceBackend` using reqwest + mTLS.
- Backend policy is modeled as `(instance, wrapper, resource, operation)`
  tuples. Rules and time-bounded grants live in a `PolicyDocument` persisted
  inside the backend store under a reserved secret id.
- The legacy SSH-forwarded Unix-socket backend transport, the legacy local
  control socket, `underwrapctl`, and `underwrap-backendctl` are gone.
- The VM test and the integration harness run the backend in service mode.

## What's still missing for first release

1. **Approval model is split across local and central state.** The backend
   knows rules/grants, but the daemon still runs a local `ApprovalState` with
   client-initiated timed unlocks. That's the transitional shape. The backend
   should own every wrapper's runtime mode. Local approval should only exist
   as "the operator is present and signed a one-time challenge right now."
2. **Dead unix-socket transport.** `BackendMode::UnixSocket`,
   `StorageConfig::Remote`, and `underwrap-store::remote::RemoteBackend` are
   still compiled even though nothing on the backend speaks that protocol.
3. **Admin API is unauthenticated.** It's safe by localhost-only today, not
   by access control.
4. **There is no operator UI.** Everything runs via direct admin HTTP calls.
5. **Docs describe the old world.** `remote-backend.md`, `architecture.md`,
   `configuration.md`, `security.md`, `testing.md`, `README.md`, `AGENTS.md`
   all still reference SSH forwarding, the control socket, `underwrapctl`, or
   the timed local unlock flow.

## Decisions frozen

- **Approval model**: backend owns per-wrapper mode
  `Locked | Unlocked { until } | PerCallApproval { approval_key }`. Local
  timed unlocks are gone. The ssh-agent challenge flow stays, but signatures
  are forwarded to the backend for central verification and audit.
- **Expiry**: `Unlocked { until }` is enforced lazily at read time and
  proactively by a background task in the backend that transitions expired
  entries back to `Locked` so that the admin UI reflects truth.
- **Backend transport**: service-only. Unix-socket mode is removed outright.
- **Admin API auth**: bearer tokens declared in backendd config.
  `Authorization: Bearer <token>` on every `/v1/admin/*` request. mTLS for
  admin is a later step.
- **UI**: separate `underwrap-admin-ui` service, not colocated in
  `underwrap-backendd`. Operator login via password + session cookie.
  Internally, the UI holds an admin API bearer token and acts as the backend's
  admin client.
- **UI stack**: the shipped operator console is the separate Next.js app in
  `apps/admin-ui`, acting as a server-side BFF in front of the backend admin
  API.
- **Docs**: collapse the three `backend-v2-*.md` drafts into one
  `docs/backend.md` describing the shipped design; rewrite the other docs
  to match.

## Milestones

### M1 — Approval model rework

Shape the backend-owned approval model.

Protocol:

- Add `WrapperApprovalMode` to `underwrap-protocol::policy`:
  - `Locked`
  - `Unlocked { until_epoch: Option<u64> }`
  - `PerCallApproval { approval_key: String }` (OpenSSH pubkey)
- Replace `WrapperPolicyState { wrapper, enabled, reason }` with
  `WrapperPolicyState { wrapper, mode, reason }`.
- Extend `PolicyDecision` to carry a `needs_approval` case that ships a
  nonce.
- Add a new authorization-aware request envelope on the service API:
  `POST /v1/read` (and friends) carries `wrapper`, `resource`, `operation`,
  and optionally `approval: { challenge_id, signature, public_key }`.
- When policy says `PerCallApproval` and no valid approval is attached, the
  backend replies `approval_required` with a fresh nonce (stored server-side
  with a short TTL) bound to `(instance, wrapper)`.
- When `approval` is attached, the backend:
  - looks up the pending nonce by `challenge_id`,
  - checks the public key matches the wrapper's stored `approval_key`,
  - verifies the ed25519/rsa signature over the nonce,
  - consumes the nonce,
  - allows the operation once and writes an audit event.

Daemon:

- Delete local `ApprovalState`, `ApprovalMode::Required`,
  `ApprovalMode::OptionalApproval`, `ApprovalPolicy.max_seconds`,
  `WrapperDef.approval_key`, and the old `/approval` wrapper message family
  for client-initiated timed unlocks.
- `underwrapd` becomes a pass-through for the approval flow: on
  `approval_required`, it relays `Challenge { nonce, challenge_id }` to the
  connected wrapper, receives `ApprovalResponse { signature, public_key }`,
  and retries the backend request with the approval attached.
- Wrapper protocol keeps a minimal `Challenge` / `ApprovalResponse` pair and
  loses every "approval policy" / "duration" field. A wrapper can only
  deliver proof of a one-time operator-present signature.

Backend expiry timer:

- `underwrap-backendd` spawns a periodic task that walks every store's
  `PolicyDocument`, transitions expired `Unlocked { until }` wrapper modes to
  `Locked` (leaving an operator-visible audit reason), and prunes stale
  pending nonces.

Tests:

- Unit tests in `underwrap-protocol::policy` for the new mode evaluation.
- Unit tests in `underwrap-backendd::server` for nonce storage, signature
  verification (ed25519 and rsa happy paths, mismatched key, bad signature,
  expired nonce), and the expiry sweeper.
- Integration tests in
  `crates/integration-tests/src/tests/wrapper_permissions.rs` (or a new
  `approval.rs`) that exercise the end-to-end per-call approval flow
  using a test ed25519 key.
- Smoke test for the VM test that sets `PerCallApproval` centrally and
  proves reads fail without a valid signature.

### M2 — Backend consolidation cleanup

- Delete `BackendMode`, collapse `BackendSocketConfig` to something like
  `BackendServiceConfig { url, ca_file, client_cert_file, client_key_file }`.
- Delete `StorageConfig::Remote`, `underwrap-store::remote::RemoteBackend`,
  the `UnixSocketBackend` alias, and the `open_backend_client` unix-socket
  arm.
- Rename `crates/underwrapd/src/unlock.rs` to `approval.rs` and
  `unlock_flow.rs` to `approval_flow.rs`; drop the `#[path = "…"]` attributes
  in `crates/underwrapd/src/lib.rs`.
- Simplify the NixOS module `backend` submodule: no `mode`, no `socket`
  option; require service fields.
- Drop unix-socket test fixtures that are no longer exercised.

Tests: all existing suites stay green with the reduced transport surface.

### M3 — Admin API bearer-token auth

- Add `admin.tokens: HashMap<String, String>` (name → opaque token string)
  to `BackendConfig`. Tokens are loaded from file paths so secrets don't need
  to sit in the YAML verbatim.
- Require `Authorization: Bearer <token>` on every `/v1/admin/*` request.
- Tag structured logs with the token's name (operator identity).
- Update the integration harness to send a bearer token. Update `node.nix`
  and `test.py` for the VM test.

Tests:

- Unit tests: token parsing, constant-time comparison, rejection of missing
  / bad / mistyped headers.
- Integration tests: existing admin harness asserts 401 without a token,
  succeeds with the right one.

### M4 — `underwrap-admin-ui` Next.js operator console

This milestone shipped as the Next.js app in `apps/admin-ui/`, not as a Rust
SSR crate.

Delivered scope:

- server-side BFF architecture in front of `underwrap-backendd`
- username/password operator auth with signed `HttpOnly` sessions
- resource explorer with reveal, upload/download, write, delete, and mutation
  flows
- wrapper permissions console
- systemd + NixOS module wiring for deployment alongside backendd

Tests:

- frontend unit tests for auth/session/CSRF helpers and route-adjacent logic
- backend tests for the UI-oriented admin API surface
- Playwright browser regressions covering protected-route/session failures,
  CSRF rejection, reveal/download no-store behavior, mutation/upload/delete,
  wrapper-mode changes and permission toggles

### M5 — Docs rewrite

- Delete `docs/backend-v2-architecture.md`,
  `docs/backend-v2-implementation-plan.md`,
  `docs/backend-v2-policy-api.md` (or move to `docs/archive/` if we want a
  paper trail — default: delete).
- Write `docs/backend.md` describing the shipped service design.
- Rewrite `docs/remote-backend.md` as "Deploying the central backend."
- Update `docs/architecture.md`, `docs/configuration.md`,
  `docs/getting-started.md`, `docs/security.md`, `docs/testing.md`,
  `docs/wrapper-author-checklist.md`, `AGENTS.md`, `README.md` to match the
  real shape.
- Add `docs/admin-ui.md` for operators.

## Test posture

Testing is part of each milestone, not a trailing phase:

- Unit tests live next to the code they cover.
- Integration tests extend `crates/integration-tests` so they share the
  backendd + underwrapd harness.
- browser/UI tests live under `apps/admin-ui/tests/e2e` and use Playwright
  against a local test stack.
- Full gate on every milestone:

  ```bash
  cargo test --workspace
  cargo fmt -- --check
  cargo clippy --all-targets --workspace -- -D warnings
  nix build .#checks.x86_64-linux.vm-test
  ```

## Open questions (tracked separately)

- Concrete encoding of the ssh signature and pubkey on the admin/runtime
  API: raw `ssh_key::Signature` bytes + OpenSSH pubkey string, base64'd. This
  gets nailed down in M1 when the request/response types land.
- Whether to log denied per-call approvals centrally even when the daemon
  never completes the retry. Default: yes; underwrapd POSTs a best-effort
  notice.
- Whether `PerCallApproval` should support multiple authorized pubkeys
  eventually. Out of scope for first release: one key per wrapper.
