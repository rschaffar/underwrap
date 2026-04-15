# Backend v2 implementation plan (draft)

This document turns the backend v2 architecture note into an execution plan.

It assumes the architectural direction in
[Backend v2 architecture (draft)](backend-v2-architecture.md):

- `underwrapd` remains the local security broker
- the backend becomes a real authenticated service
- the target process does not talk to the backend directly
- backend namespace selection comes from authenticated instance identity
- backend owns central wrapper/resource authorization policy and grants
- the current local unlock flow is transitional and removable
- the current SSH-forwarded Unix-socket remote model is transitional and removable

Because underwrap is still greenfield, this plan optimizes for the clean target
architecture, not for preserving the current remote model indefinitely.

## Status

**Planning draft. Not implemented yet.**

## High-level objective

Move from this remote architecture:

```text
wrapper -> underwrapd -> target-local backend socket -> SSH forward -> backendd
```

to this one:

```text
wrapper -> underwrapd -> authenticated backend service
```

while preserving underwrap's core invariants:

- wrapper identity remains local and kernel-backed
- all secret material still flows through the backend/storage model
- no backend credentials are leaked to the final target process
- no degraded fallback to weaker security modes

## Scope and sequencing

The implementation should be done in clearly separated phases.

The phases below are ordered to reduce risk:

1. freeze the protocol/config direction
2. make backend access transport-neutral inside the codebase
3. implement the backend service transport and identity model
4. switch local and VM deployments to the new model
5. remove the old socket-export remote mode

This avoids trying to rewrite transport, auth, config, tests, and docs in one step.

---

## Phase 0 — freeze decisions before coding

### Goal

Resolve the minimum open questions needed to prevent churn during implementation.

### Decisions to freeze

1. **Transport shape**
   - Adopt a small underwrap-native **HTTPS API** for backend v2.
   - Do not start with gRPC.

2. **Client identity**
   - Backend authenticates the `underwrapd` instance principal.
   - Requests do not include trusted namespace/store selectors.

3. **Initial auth model**
   - Use **mTLS** for `underwrapd` -> backend.
   - Do not start with bearer tokens.

4. **Authorization model**
   - Backend authorizes on `(instance, wrapper, resource, operation)`.
   - `underwrapd` supplies the trusted wrapper identity after local kernel-backed authentication.
   - Local wrapper config remains a ceiling; backend policy/grants are the runtime control plane.

5. **Unlock replacement**
   - Replace the current local unlock flow with central backend policy/grants.
   - Do not preserve SSH-signature-based local unlock as a first-class v2 feature.

6. **Migration posture**
   - The old SSH/socket remote model may coexist briefly during development.
   - It is not a long-term compatibility commitment.

### Deliverable

A short accepted design note or issue comment freezing these decisions.

### Exit criteria

- No unresolved debate about REST vs gRPC
- No unresolved debate about mTLS vs token auth for v1
- No unresolved debate about whether the backend sees wrapper identity directly
- No unresolved debate about replacing local unlock with central grants

---

## Phase 1 — refactor backend access into a transport-neutral client layer

### Goal

Stop modeling backend access as "a Unix socket" in the main abstractions.

### Why this phase exists

Right now the code and config are shaped around the socket transport:

- `underwrapd` opens `underwrap_store::RemoteBackend`
- `StorageConfig::Remote` points at a Unix socket path
- test harnesses and VM tests create backend export sockets explicitly

That makes the service redesign harder than it needs to be.

### Main changes

#### 1. Introduce a transport-neutral backend client configuration

Evolve the protocol/config model from something conceptually like:

```yaml
backend:
  socket: /run/underwrap-<instance>/backend/backend.sock
```

toward something conceptually like:

```yaml
backend:
  mode: service
  url: https://backend.example.internal
  tls: ...
  auth: ...
```

During migration, an enum is fine, for example:

- `unix-socket`
- `service`

The exact schema can be refined later, but the code should stop assuming the
backend is necessarily a socket path.

#### 2. Split the current remote backend implementation by transport semantics

Today `underwrap_store::RemoteBackend` really means:

- request/response backend client over Unix socket

Refactor this into clearer names, for example:

- `UnixSocketBackendClient`
- `HttpBackendClient`
- `BackendClient` trait or equivalent abstraction

The exact naming can differ, but the important part is to stop baking the old
transport into the central type name.

#### 3. Keep the storage operation surface small

Retain the current core operations for the first pass:

- read
- write
- list
- delete

That keeps the transport migration contained.

#### 4. Keep local wrapper flows unchanged

This phase must not change:

- wrapper socket auth
- fd passing
- libsecret local mechanics
- directory channel local mechanics

Only the backend client boundary should move.

### Files likely affected

- `crates/underwrap-protocol/src/config.rs`
- `crates/underwrap-store/src/lib.rs`
- `crates/underwrap-store/src/remote.rs` (likely split/renamed)
- `crates/underwrapd/src/server.rs`
- `crates/underwrapd/src/storage.rs`
- config/docs referencing backend socket shape

### Tests to add/update

#### Unit tests

- config parsing for new backend client enum/config
- opening the correct backend client implementation from config
- compatibility tests for temporary dual-mode config if retained during migration

#### Integration tests

- existing harness continues to work against the Unix-socket transport during this phase
- add tests that exercise the transport-neutral client abstraction itself

### Exit criteria

- `underwrapd` can be configured against an abstract backend client mode
- current tests still pass with the old socket transport
- no major code path outside backend client setup assumes Unix socket transport

---

## Phase 2 — define the backend v2 service API, policy model, and identity binding

### Goal

Define the concrete service contract before implementing it.

### API principles

1. **Authenticated identity selects namespace**
   - no trusted `store` field in the request body

2. **Operations are authorization-aware**
   - requests include the trusted wrapper identity
   - backend authorizes the actual operation, not a detached preflight

3. **Keep the surface small**
   - health/status
   - read
   - write
   - list
   - delete
   - policy/grant inspection or mutation endpoints only if they are part of the same service boundary

4. **Transport-level mTLS carries principal identity**
   - request bodies stay simple

5. **Do not put local wrapper auth in the backend API**
   - wrapper auth remains entirely local to `underwrapd`

### Backend request model

The backend requests should be shaped around the real authorization tuple:

```text
(instance identity from mTLS, trusted wrapper, resource, operation)
```

So conceptually the API is not just:

- read secret id
- write secret id

It is more like:

- read secret id as wrapper X
- write secret id as wrapper X
- open libsecret session as wrapper X
- open directory session as wrapper X

The implementation may internally separate authorization from storage, but the
public backend API should keep them tied together.

### Proposed initial endpoint set

Conceptually:

- `GET /v1/status`
- `POST /v1/secrets/read`
- `POST /v1/secrets/write`
- `POST /v1/secrets/delete`
- `POST /v1/secrets/list`

Using `POST` for most stateful operations is acceptable here if it keeps the
request body explicit about wrapper/resource/operation metadata.

### Identity binding rules

The backend must:

- authenticate the client certificate
- extract the instance identity from cert subject/SAN or equivalent
- map that identity to one backend namespace/store view
- reject requests without a valid mapping

### Policy model to define here

Backend-side policy likely needs both:

- long-lived baseline rules
- temporary grants or disablements

Conceptually:

```yaml
clients:
  underwrap:personal:work@vm-01:
    store: work
    policy:
      - wrapper: gh-comment
        resource: github/token
        operation: read
        effect: allow
      - wrapper: aws-prod
        resource: aws/prod
        operation: read
        effect: deny
```

The exact config format is TBD, but this phase should freeze the conceptual
policy tuple and how temporary grants relate to baseline rules.

### Files likely added/affected

- `crates/underwrap-protocol` if request/response structs are shared
- `crates/underwrap-backendd` new service API modules
- docs for configuration and architecture

### Tests to add/update

#### Unit tests

- namespace mapping from authenticated principal
- policy evaluation for `(instance, wrapper, resource, operation)`
- reject unknown client principals
- reject wrapper/resource combinations not allowed by policy
- request parsing/serialization if shared types are used

#### Integration tests

- in-process or loopback tests for backend HTTP handlers
- identity-to-store binding tests
- policy allow/deny tests for different wrappers on the same instance
- negative tests for unauthorized principals

### Exit criteria

- API shape documented
- principal-to-namespace mapping documented
- wrapper/resource policy model documented
- auth binding rules documented
- test scaffolding for service handlers exists

---

## Phase 3 — implement backend service mode in `underwrap-backendd`

### Goal

Turn `underwrap-backendd` into the real authenticated backend service for v2.

### Main changes

#### 1. Add service listener support

`underwrap-backendd` should accept network service configuration, likely:

- bind address / port
- server TLS certificate and key
- client CA / trust bundle for mTLS

#### 2. Add authenticated and policy-aware request handling

For each request:

- verify TLS peer identity
- map identity to namespace/store
- evaluate `(instance, wrapper, resource, operation)` against backend policy/grants
- execute the storage operation only if authorized
- return minimal structured response

#### 3. Preserve storage backend reuse

The service should reuse the existing storage implementations where possible:

- plain backend
- encrypted backend

The service transport changes; the storage implementations should not need a rewrite.

#### 4. Keep management responsibilities separate

The old management socket/export functionality should not be expanded further.

During migration it may temporarily coexist, but new design effort should go into:

- service listener config
- client principal mapping
- service health/audit

#### 5. Add backend-side audit logging

The backend should log:

- authenticated instance principal
- trusted wrapper name
- requested resource and operation
- allow/deny decision
- result status

It must continue to avoid logging secret material.

### Files likely affected

- `crates/underwrap-backendd/src/server.rs`
- new backendd HTTP/TLS modules
- backendd config parsing
- `crates/underwrap-backendd/src/main.rs`

### Tests to add/update

#### Unit tests

- TLS config validation
- principal extraction/mapping
- policy evaluation and grant precedence
- API handler behavior for read/write/list/delete/status

#### Integration tests

- start backendd in service mode on loopback
- send authenticated requests using test certs
- verify isolation between two client principals
- verify different wrapper policies on the same principal
- verify unknown or invalid client is rejected
- verify denied wrapper/resource operations never reach storage

### Exit criteria

- backendd serves the v2 service API on loopback in tests
- authenticated principal is required for all stateful operations
- wrapper/resource policy is enforced before storage access
- existing storage operations work through the service transport

---

## Phase 4 — implement service-mode client in `underwrapd`

### Goal

Allow `underwrapd` to talk to the backend service directly.

### Main changes

#### 1. Add `HttpBackendClient` / service client implementation

This client should:

- establish HTTPS connections to backendd
- authenticate with configured client cert/key
- support read/write/list/delete/status
- map errors into the existing daemon behavior cleanly

#### 2. Update frontend config handling

`underwrapd` should be able to start in service-backend mode from config.

During migration it may still support the old socket mode, but new deployments
should be able to use only the service mode.

#### 3. Keep local daemon behavior mostly unchanged

Wrapper flows should continue to behave the same from the user's perspective except for the replacement of local unlock with central policy/grants:

- same wrapper invocation
- same secretsd and directory flows
- same local wrapper identity checks
- central policy decides allow/deny instead of the old local unlock flow
- only the backend/policy control plane changed

### Files likely affected

- `crates/underwrap-store` service client implementation
- `crates/underwrapd/src/server.rs`
- frontend config loading/validation
- NixOS module generation for backend config

### Tests to add/update

#### Unit tests

- service client request construction
- error mapping and reconnect/retry behavior as designed
- config validation for service backend mode

#### Integration tests

- full daemon + backendd service-mode harness
- wrapper request succeeds through service transport when policy allows it
- wrapper request is denied when backend policy revokes it
- control socket store/status flows succeed through service transport

### Exit criteria

- `underwrapd` can run end to end against backendd service mode in automated tests
- wrapper/control behavior is unchanged except for transport and the move from local unlock to central grants

---

## Phase 5 — migrate local development and VM tests to the new backend model

### Goal

Make the real deployment and test story validate the new architecture, not the old one.

This phase is critical.

The backend redesign is not done until the NixOS VM test exercises it end to end.

### NixOS module changes

Update the module so frontend instances can be configured against a backend service.

The exact final split is a product decision, but the test deployment should model the intended architecture clearly.

Likely options:

- service config generated directly for test nodes
- a simple colocated backend service bound on loopback for the local instance
- a second backend service identity / namespace for the remote-like instance

The important thing is that the VM test no longer proves the SSH-forward socket story as the main remote path.

### VM test redesign

The current VM test has explicit coverage for:

- backend export services
- SSH tunnel establishment
- forwarded backend socket lifecycle
- local lock/unlock lifecycle

Those parts must be replaced with service-mode coverage.

#### Replace these checks

Current checks to remove or rewrite:

- `underwrap-backend-export.service`
- `underwrap-backend-export-local.service`
- SSH key provisioning purely for backend forwarding
- presence/absence of `/run/underwrap-<instance>/backend/backend.sock` as the remote contract
- tunnel up/down behavior as the remote availability model
- local lock/unlock lifecycle as the main access-control story

#### Add these checks instead

New VM assertions should include:

1. **Backend service boot and health**
   - backend service starts with service listener enabled
   - configured trust material exists with expected permissions

2. **Authenticated instance connectivity**
   - `underwrapd` connects outbound to backend service successfully
   - local wrapper invocation succeeds through service transport

3. **Central wrapper/resource authorization**
   - wrapper A may access secret X when backend policy allows it
   - wrapper A is denied secret X after central revocation
   - wrapper B on the same instance can be allowed or denied independently
   - temporary grants with expiry are validated if v1 includes them

4. **Identity binding isolation**
   - instance A can read only its mapped namespace
   - instance B cannot read instance A secrets
   - logs show principal-bound backend access, without secret values

5. **Failure behavior**
   - wrong or missing client credential causes backend access failure
   - backend unavailable causes wrapper failure without degraded fallback

6. **End-to-end libsecret and directory flows over service transport**
   - current libsecret VM assertions still pass
   - current directory staging/sync assertions still pass
   - same-UID host-side isolation checks still pass

7. **No secret leakage in logs**
   - continue current log assertions
   - include backend service logs in the scan

### Test files likely affected

- `nix/tests/node.nix`
- `nix/tests/test.py`
- any helper scripts or env wiring for the VM test

### Exit criteria

- VM test validates backend v2 service mode end to end
- no VM test depends on SSH socket forwarding for the main remote architecture story
- libsecret and directory assertions still pass through the new backend transport

---

## Phase 6 — update Rust integration harnesses to the service model

### Goal

Make the non-VM integration tests cover the new transport too.

### Current issue

The current harness in `crates/integration-tests/src/harness.rs` starts:

- backendd management socket
- backend export socket
- underwrapd against that exported Unix socket

That directly encodes the old model.

### Plan

#### 1. Add a service-mode harness path

The harness should be able to start:

- backendd in service mode
- underwrapd configured with service backend mode
- test certificates/trust material

#### 2. Keep dual harness support briefly if useful

For migration, it is fine to keep:

- old socket harness
- new service harness

But all new coverage should target the service harness.

#### 3. Port the most important scenarios first

Priority scenarios:

- read via wrapper socket when policy allows it
- deny via wrapper socket when policy revokes it
- store/status via control socket
- libsecret channel and directory channel still function correctly

### Tests to add/update

- service-harness startup/shutdown
- authenticated backend access through daemon
- negative cert/auth cases
- cross-instance isolation tests if harness supports multiple principals

### Exit criteria

- integration harness supports backend service mode
- at least the main daemon integration flows run through the new harness

---

## Phase 7 — update docs and operator story

### Goal

Make the docs reflect the real intended architecture.

### Docs to update

At minimum:

- `docs/architecture.md`
- `docs/remote-backend.md`
- `docs/configuration.md`
- `docs/getting-started.md`
- `docs/testing.md`
- `docs/security.md`

### Main documentation changes

- describe backend as a service, not a forwarded socket
- describe backend auth in terms of instance identity
- describe central wrapper/resource authorization and grants
- remove the old local unlock model from the main story
- remove tunnel lifecycle as the main remote model
- update testing docs to mention the new VM assertions

### Exit criteria

- the docs describe the implemented architecture, not the retired one
- no primary doc still presents SSH forwarding as the intended backend design

---

## Phase 8 — remove the old remote socket-export architecture

### Goal

Delete the transitional design once the new one is proven.

### Removal targets

Likely candidates:

- backend export workflow as the primary remote path
- remote backend docs centered on SSH forwarding
- socket-export-specific VM setup
- old harness code once no longer needed
- old config schema fields that only exist for socket transport

### Caution

Only do this after:

- workspace tests pass
- integration tests pass
- VM test passes
- docs are updated

### Exit criteria

- one clear backend story remains
- no maintenance burden from carrying both architectures indefinitely

---

## Cross-cutting testing plan

Testing is part of the implementation, not cleanup afterward.

## Required command gates during the project

For meaningful milestones, run:

```bash
cargo test --workspace
cargo fmt -- --check
cargo clippy --all-targets --workspace -- -D warnings
nix build .#checks.x86_64-linux.vm-test
```

For milestone or merge-level validation, prefer:

```bash
nix flake check
```

## Required test updates by layer

### 1. Unit tests

Must cover:

- config parsing/validation for backend service mode
- backend principal mapping
- client request construction and error mapping
- backend handler logic

### 2. Rust integration tests

Must cover:

- daemon -> backend service transport
- wrapper/control flows through the new transport
- negative auth cases

### 3. VM test

Must cover:

- the actual deployment model intended for users
- backend service startup and connectivity
- identity-based isolation
- failure behavior without fallback
- libsecret and directory flows over the new transport

### 4. Regression expectations

No backend-v2 milestone is complete if it regresses:

- wrapper identity checks
- central wrapper/resource authorization behavior
- libsecret mode
- directory credential mode
- log secrecy guarantees
- existing isolation assertions

---

## Suggested milestone breakdown

A practical milestone sequence would be:

### Milestone 1

- freeze design decisions
- transport-neutral backend client abstraction
- no behavior change yet

### Milestone 2

- backendd service API
- service config, principal mapping, and wrapper/resource policy model
- unit/integration coverage for backendd service mode

### Milestone 3

- `underwrapd` service-mode client
- central policy/grant enforcement wired through daemon requests
- daemon integration tests through service mode

### Milestone 4

- NixOS module and VM test switched to backend service mode
- central authorization/revocation VM coverage added
- libsecret and directory VM coverage preserved

### Milestone 5

- docs updated
- old SSH/socket remote model removed

---

## Risks and mitigations

## Risk: transport rewrite leaks into local wrapper security logic

Mitigation:

- keep Phase 1 transport-neutral refactor narrow
- do not change wrapper auth paths during transport work

## Risk: auth and policy design churn blocks progress

Mitigation:

- freeze mTLS + instance identity + `(instance, wrapper, resource, operation)` policy model before implementing the service

## Risk: VM test becomes temporarily weaker

Mitigation:

- treat VM parity as a milestone requirement, not optional follow-up
- do not remove old VM coverage until equivalent new coverage exists

## Risk: libsecret or directory flows break subtly over the new transport

Mitigation:

- preserve those test cases in integration and VM layers
- explicitly list them as must-pass exit criteria for migration milestones

---

## Concrete next task after this plan

The next implementation task should be:

> **Phase 0 + Phase 1 design patch**
>
> - freeze the service transport, identity, and policy choices
> - freeze removal of local unlock in favor of central grants
> - introduce a transport-neutral backend client abstraction
> - rename/refactor the current Unix-socket remote backend implementation accordingly
> - add config scaffolding for dual-mode backend client config

That is the right first code move because it prepares the tree for backend v2
without yet forcing the full service rollout in one patch.
