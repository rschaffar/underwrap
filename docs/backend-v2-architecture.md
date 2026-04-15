# Backend v2 architecture (draft)

This note captures the planned direction for underwrap's next backend architecture.

It is intentionally a **design note**, not a description of the current implementation.
The current tree still uses the socket-export and SSH-forward model described in
[Remote backend](remote-backend.md).

Because underwrap is still greenfield and aiming for a polished first release,
this document optimizes for a cleaner architecture rather than backwards
compatibility with the current remote-backend shape.

## Status

**Draft design direction. Not implemented yet.**

## Why change the current remote backend model?

The current remote mode works, but it is awkward in ways that are architectural,
not just ergonomic:

- the frontend always expects a Unix socket path on the target host
- remote mode is expressed as SSH forwarding rather than as a first-class backend service
- store selection is coupled to socket export and forwarding topology
- backend availability and unlock semantics are partially entangled with tunnel lifecycle
- the remote transport is harder to operate, observe, and secure as a proper service boundary

That model made early experimentation easy, but it is not the cleanest shape for
what underwrap is becoming.

## Product position

underwrap should position itself as:

> a **local separation and authorization layer** for running ordinary CLI tools,
> while the actual secret store may live somewhere central.

That means:

- the wrapped target still runs locally on a hardened NixOS machine
- `underwrapd` still owns local wrapper authentication and policy
- a backend service may run centrally and hold the real secret state
- the target process does **not** become a direct backend client

## Architecture summary

The target architecture is:

```text
wrapper -> underwrapd (local broker) -> central backend service
```

Not:

```text
wrapper -> target-local socket -> SSH forward -> backend socket
```

## Goals

The backend v2 redesign should achieve these goals.

### 1. Make the backend a real network service

Remote storage should be modeled as a first-class backend service, not as a
Unix-socket export that happens to be forwarded elsewhere.

### 2. Keep the local security boundary in `underwrapd`

`underwrapd` should remain the only process on the target machine that talks to
the backend.

It continues to own:

- wrapper authentication from kernel identity
- local unlock policy, unless explicitly moved later
- scoped channel creation for libsecret and directory flows
- local audit of wrapper use

### 3. Bind backend access to authenticated instance identity

Backend namespace or store selection must come from authenticated client
identity, not from a client-supplied store name in the request body.

This preserves the spirit of the current socket-selected store model while
moving it to a cleaner network architecture.

### 4. Support centrally hosted secret state

The backend should be able to hold:

- simple secret blobs
- libsecret-managed state
- backend-managed directory/file state
- future credential state that still follows underwrap's storage invariants

### 5. Keep the target process out of the backend trust boundary

Wrapped tools should not receive backend credentials or talk to the backend
service directly.

### 6. Improve operational clarity

Operators should be able to reason about:

- where the backend service runs
- which target instance is authenticated
- which namespace that instance can access
- whether the service is healthy
- how access is audited

without reverse-engineering an SSH forwarding topology.

## Non-goals

This redesign does **not** aim to turn underwrap into a generic enterprise
secret manager or a standards-compatibility layer for every existing backend.

Specifically, backend v2 is not trying to:

- expose a generic secret-store protocol used by many unrelated products
- let wrapped target tools authenticate to the backend directly
- replace the local wrapper-authentication model with remote auth
- become a general behavior sandbox for target processes
- preserve the SSH-forwarded socket design for compatibility if a cleaner design wins

## Trust boundaries

The important boundaries in backend v2 are:

### Local boundary: wrapper -> `underwrapd`

This remains the security-critical local boundary.

Authentication stays based on:

- per-wrapper socket selection
- `SO_PEERCRED`
- effective GID validation from `/proc/<pid>/status`

### Remote boundary: `underwrapd` -> backend service

This becomes the main backend trust boundary.

The backend does not know or trust wrapper identity directly. It trusts the
authenticated `underwrapd` instance and the namespace/instance mapping attached
to that identity.

### Final target process boundary

The wrapped target remains outside the backend trust boundary.

The target may receive secret material or talk to `underwrap-secretsd`, but it
must not inherit backend credentials or a direct backend capability.

## Main actors

### `underwrapd`

Role:

- local broker and policy engine on the target VM

Responsibilities:

- authenticate wrappers locally
- enforce local wrapper policy
- mediate backend access
- create scoped channels for libsecret and directory flows
- audit local secret use

### backend service (`underwrap-backendd` v2)

Role:

- central authenticated storage and policy service

Responsibilities:

- authenticate `underwrapd` instances
- map instance identity to backend namespace
- read/write/list/delete secret state
- persist backend-managed mutable state
- provide backend-side audit and health reporting

### wrapped target process

Role:

- consumer of locally brokered secret access

Responsibilities:

- none at the backend layer

The target process should remain unaware of backend credentials.

## Proposed identity model

## Instance identity

The backend should authenticate a **target instance principal**, not a human user
and not an individual wrapped tool.

Recommended principal shape:

```text
underwrap:<deployment-or-tenant>:<instance>@<machine>
```

Examples:

```text
underwrap:personal:work@vm-01
underwrap:corp:gh@builder-02
```

The exact string format is less important than the rule that one authenticated
principal maps to one backend namespace or policy set.

## Authentication recommendation

Recommended default direction:

- **mTLS** between `underwrapd` and backend service
- one client credential per target instance
- short-lived credentials if operationally feasible
- backend-pinned CA or trust bundle on the client side

This avoids static bearer tokens and keeps proof of possession bound to the TLS
channel.

## SPIFFE compatibility

Underwrap should not require SPIFFE for v1 of backend v2, but the identity model
should be compatible with adopting SPIFFE-style workload identity later.

That means:

- keep identity certificate-centric
- avoid request-header auth schemes as the primary design
- keep namespace mapping backend-side
- prefer short-lived client credentials over static shared secrets

## Authorization model

The backend should authorize based on the authenticated instance principal.

### Core rule

**The client does not choose the namespace/store in the request body.**

Instead:

- backend authenticates the client principal
- backend maps that principal to a namespace or store view
- requests operate only within that mapped namespace

That means requests can stay simple:

- read secret by id
- write secret by id
- list ids by prefix
- delete secret by id

without adding a trusted `store` or `namespace` parameter supplied by the
client.

## Local unlock vs backend authorization

Backend v2 should begin with a simple split:

- `underwrapd` keeps local wrapper unlock policy
- backend enforces only instance-level authentication and namespace authorization

A backend-side lease or unlock concept may be added later if it serves a clear
purpose, but it is not required to make the architecture clean.

## Backend data model

The backend should remain underwrap-shaped, not forced into a generic KMS model.

## Base object model

Recommended base abstraction:

- **namespace**
  - selected by authenticated instance identity
- **secret id**
  - logical key within the namespace
- **binary value**
  - opaque bytes
- **optional metadata**
  - timestamps, version/etag, content type, internal kind

The minimum v2 storage surface should still support the current storage trait shape:

- read
- write
- list
- delete

## Representation of richer state

Richer modes can still be represented on top of that base model:

### simple env secrets

Store as one secret id -> one opaque value.

### libsecret mode

Persist the libsecret data model in backend-controlled records under a reserved
prefix, rather than inventing a separate side store outside the backend model.

### directory credentials

Persist directory trees as backend-managed file objects under reserved prefixes,
or evolve the backend trait to include file-tree primitives if that becomes
clearer than encoding them as flat object ids.

For the first backend v2 design pass, the important thing is not the exact
encoding but the invariant:

> all secret material and backend-managed credential state still flows through the
> backend service and its storage model.

## Transport candidates

The architectural change comes first. Transport choice comes after the trust and
data model are clear.

Still, the likely candidates are:

### Option A: HTTPS + JSON API

Pros:

- easiest to debug and operate
- easy health checks and observability
- minimal protocol machinery
- good fit for the current small CRUD-like storage surface

Cons:

- less strongly typed than gRPC
- streaming patterns are less ergonomic if needed later

### Option B: gRPC

Pros:

- strong typed RPC model
- good if the API grows many methods or streaming workflows
- natural fit for some identity ecosystems

Cons:

- more tooling and codegen complexity
- worse ergonomics for simple inspection and manual debugging
- unnecessary weight if the backend API stays small

## Recommendation

Prefer **HTTPS + a small underwrap-native API** for the first backend v2
implementation.

The important protocol decision is not REST vs gRPC; it is:

- authenticated instance identity
- backend-side namespace binding
- no static bearer secrets as the core auth model

## Operational model

Backend v2 should be easy to explain operationally:

- target VM runs `underwrapd`
- central backend runs as a network service
- `underwrapd` connects outbound to the backend
- backend authenticates the instance
- backend serves only that instance's namespace/policy view

This is a cleaner fit for hardened NixOS-managed VMs than requiring operators to
materialize a target-local socket path via SSH forwarding.

## Configuration direction

The current frontend config points to a backend Unix socket path.

Backend v2 will likely need a new config shape conceptually like:

```yaml
backend:
  url: https://backend.example.internal
  auth:
    # exact format TBD
  tls:
    # exact format TBD
```

The exact schema can wait until the identity model is frozen.

## Migration strategy

Because the project is greenfield, the migration should optimize for the clean
result, not for preserving every intermediate shape.

Recommended staged plan:

### Phase 1: architectural freeze

Adopt this architecture direction in docs and planning:

- central backend service
- `underwrapd` as sole backend client on target hosts
- backend namespace selected by authenticated instance identity

### Phase 2: backend abstraction cleanup

Reshape the storage/client abstractions so the transport is no longer modeled as
"a remote Unix socket" specifically.

For example:

- keep the storage operations simple
- introduce a transport-neutral remote backend client abstraction
- decouple backend namespace selection from socket export

### Phase 3: implement backend v2 transport

Implement a network service transport for the backend and a matching client in
`underwrapd`.

### Phase 4: temporary coexistence if useful

If it meaningfully lowers migration risk during development, keep both:

- current socket/SSH remote mode
- new service-based backend mode

behind separate config shapes while the new design is validated.

### Phase 5: remove the socket-export remote model

Once the service-based backend is proven, remove the SSH-forwarded socket export
model rather than carrying both designs indefinitely.

## Open questions

These questions still need explicit decisions.

### 1. Exact instance identity format

What precise principal string or certificate subject/SAN format should identify
an `underwrapd` instance?

### 2. Credential bootstrap

How does a newly provisioned target VM obtain its first backend client
credential?

### 3. Backend-side leases

Should the backend eventually gain an explicit lease/unlock concept, or should
instance authentication plus local wrapper unlock remain the whole model?

### 4. Rich-state encoding

Should directory and libsecret state remain encoded over the current simple
storage trait, or should backend v2 add structured primitives for them?

### 5. Multi-writer behavior

If multiple target instances can legitimately access the same logical namespace,
how are concurrent writes or state merges handled?

## Proposed decisions for the next planning step

To keep the redesign moving, the next planning session should aim to freeze
these decisions first:

1. `underwrapd` is the only backend client on target machines
2. backend is a central authenticated network service
3. backend namespace selection comes from authenticated instance identity
4. local wrapper unlock remains in `underwrapd` for the first version
5. backend v2 starts with a small underwrap-native HTTPS API
6. SSH-forwarded socket export remote mode is considered transitional and removable

If those six are accepted, the next design note should specify:

- exact instance identity and auth bootstrap
- concrete backend API surface
- config schema changes
- migration of `underwrap-store::RemoteBackend` into a transport-neutral client model
