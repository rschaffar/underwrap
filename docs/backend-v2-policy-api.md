# Backend v2 policy and API schema (draft)

This note makes the backend v2 authorization model more concrete.

It builds on:

- [Backend v2 architecture (draft)](backend-v2-architecture.md)
- [Backend v2 implementation plan (draft)](backend-v2-implementation-plan.md)

It focuses on two things:

1. the **policy model** the backend should enforce
2. the **runtime API** between `underwrapd` and the backend service

This is still a planning document. It is intended to be concrete enough to guide
implementation without yet freezing every field name forever.

## Design summary

The backend should authorize requests using this tuple:

```text
(instance, wrapper, resource, operation) -> allow/deny
```

Where:

- **instance** comes from backend-authenticated client identity (for example mTLS)
- **wrapper** is asserted by trusted `underwrapd` after local kernel-backed authentication
- **resource** identifies the secret or backend-managed state object
- **operation** identifies the kind of access requested

The key split is:

- `underwrapd` proves **who the wrapper is**
- the backend decides **what that wrapper may do right now**

## Core model

### 1. Instance principal

The backend authenticates an `underwrapd` instance principal.

Example identifiers:

```text
underwrap:personal:work@vm-01
underwrap:corp:gh@builder-02
```

The exact encoding may live in certificate SANs or another mTLS-bound field,
but the backend must end up with one stable instance principal string.

### 2. Wrapper name

This is the local wrapper identity already authenticated by `underwrapd`.

Examples:

```text
gh-comment
gh-auth
aws-prod
jira-readonly
```

The backend trusts this field only because the caller is a trusted
`underwrapd` instance.

### 3. Resource

A resource is any backend-managed secret or state object that can be authorized.

Recommended canonical form:

```text
<kind>:<id>
```

Examples:

```text
secret:github/token
secret:jira/token
libsecret:gh/default
directory:test/dir-a
```

This makes policy explicit across different access modes.

### 4. Operation

Operations should be explicit and narrow.

Recommended starting set:

#### Plain secret operations

- `secret.read`
- `secret.write`
- `secret.delete`
- `secret.list`

#### Libsecret operations

- `libsecret.read`
- `libsecret.write`
- `libsecret.readwrite`

#### Directory operations

- `directory.read`
- `directory.write`
- `directory.readwrite`

The implementation may normalize some of these internally. For example,
`libsecret.readwrite` may imply both read and write.

## Policy layers

Authorization should be the intersection of two layers.

### Layer A: local wrapper ceiling

The local underwrap configuration defines the maximum resource references a
wrapper is ever allowed to attempt.

Examples:

- `gh-comment` may use only `github/token`
- `gh-auth` may use only `libsecret:gh/default`
- `dir-sync-a` may use only `directory:test/dir-a`

This remains local and enforced by `underwrapd`.

### Layer B: backend runtime policy/grants

The backend decides whether that locally valid access is currently allowed.

Examples:

- `gh-comment` currently allowed to read `secret:github/token`
- `gh-auth` currently allowed to readwrite `libsecret:gh/default`
- `aws-prod` currently denied entirely
- `jira-readonly` temporarily enabled for 30 minutes

### Effective rule

A request is allowed only if:

- local wrapper config allows the reference, and
- backend policy/grants allow the request tuple

## Policy objects

### Baseline rule

A baseline rule is long-lived policy.

Suggested shape:

```json
{
  "wrapper": "gh-comment",
  "resource": "secret:github/token",
  "operations": ["secret.read"],
  "effect": "allow"
}
```

Fields:

- `wrapper`: wrapper name
- `resource`: canonical resource identifier
- `operations`: one or more operations covered by the rule
- `effect`: `allow` or `deny`

These rules are evaluated within one instance namespace.

### Grant

A grant is dynamic, operator- or UI-driven runtime state.

Suggested shape:

```json
{
  "id": "grant-01HV...",
  "wrapper": "aws-prod",
  "resource": "secret:aws/prod",
  "operations": ["secret.read"],
  "effect": "allow",
  "starts_at": "2026-04-15T12:00:00Z",
  "expires_at": "2026-04-15T12:30:00Z",
  "reason": "temporary production deploy",
  "created_by": "alice@example.org"
}
```

Fields:

- `id`: stable grant identifier
- `wrapper`, `resource`, `operations`, `effect`: same semantics as rules
- `starts_at`: optional activation time
- `expires_at`: optional expiry
- `reason`: operator-visible explanation
- `created_by`: audit field

### Wrapper disable switch

For convenience, a coarse wrapper-level switch is useful.

Suggested shape:

```json
{
  "wrapper": "aws-prod",
  "enabled": false,
  "reason": "disabled pending review"
}
```

This avoids expressing a deny rule for every resource when the operator simply
wants a wrapper turned off.

## Policy evaluation order

A simple and understandable order is:

1. authenticate instance principal
2. map principal to namespace
3. confirm local request shape is valid
   - wrapper present
   - resource present
   - operation present
4. evaluate wrapper enabled/disabled state
5. evaluate active deny rules / deny grants
6. evaluate active allow grants
7. evaluate baseline allow rules
8. default deny

### Recommended precedence

Recommended precedence:

```text
wrapper disabled
  > explicit deny grant/rule
  > explicit allow grant
  > explicit allow baseline rule
  > default deny
```

That gives operators an intuitive override story.

## Runtime API: `underwrapd` -> backend

The runtime API should be **authorization-aware**.

That means `underwrapd` does not first call `authorize()` and then call an
unscoped storage API. Instead, it calls one operation that includes the trusted
wrapper identity and requested action.

## Common request envelope

A useful request envelope is:

```json
{
  "wrapper": "gh-comment",
  "resource": "secret:github/token",
  "operation": "secret.read",
  "scope": null,
  "audit": {
    "peer_uid": 1099,
    "target_kind": "wrapper"
  }
}
```

Fields:

- `wrapper`: trusted wrapper identity from local auth
- `resource`: canonical backend resource id
- `operation`: requested operation
- `scope`: optional scope string if needed by wrapper semantics
- `audit.peer_uid`: optional local uid for audit context only
- `audit.target_kind`: optional classifier for audit readability

The instance principal itself should come from mTLS, not from the JSON body.

### 1. Status

```http
GET /v1/status
```

Response:

```json
{
  "ok": true,
  "instance": "underwrap:personal:work@vm-01"
}
```

### 2. Read secret

```http
POST /v1/secrets/read
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "gh-comment",
  "resource": "secret:github/token",
  "operation": "secret.read"
}
```

Successful response:

```json
{
  "allowed": true,
  "material_b64": "Z2hwX3Rlc3Q="
}
```

Denied response:

```json
{
  "allowed": false,
  "error": {
    "code": "access_denied",
    "message": "wrapper 'gh-comment' may not perform secret.read on secret:github/token"
  }
}
```

### 3. Write secret

```http
POST /v1/secrets/write
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "gh-auth",
  "resource": "secret:github/token",
  "operation": "secret.write",
  "material_b64": "dXBkYXRlZA=="
}
```

Response:

```json
{
  "allowed": true,
  "written": true
}
```

### 4. Delete secret

```http
POST /v1/secrets/delete
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "gh-admin",
  "resource": "secret:github/token",
  "operation": "secret.delete"
}
```

Response:

```json
{
  "allowed": true,
  "deleted": true
}
```

### 5. List resources under a prefix

```http
POST /v1/secrets/list
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "ops-list",
  "resource": "secret:github",
  "operation": "secret.list"
}
```

Response:

```json
{
  "allowed": true,
  "ids": [
    "secret:github/token",
    "secret:github/user"
  ]
}
```

## Session-oriented runtime operations

Some underwrap features are session-like rather than single read/write calls.

Two obvious examples are:

- libsecret mode
- directory credential mode

For those, the backend API should still be authorization-aware.

### Libsecret session open

```http
POST /v1/libsecret/open-session
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "gh-auth",
  "resource": "libsecret:gh/default",
  "operation": "libsecret.readwrite"
}
```

Response:

```json
{
  "allowed": true,
  "session_id": "lsess_01HV...",
  "capabilities": ["read", "write"]
}
```

This session id is backend-issued scoped state for further libsecret RPCs over
a channel or subsequent HTTP calls, depending on final implementation.

### Directory session open

```http
POST /v1/directories/open-session
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "dir-sync-a",
  "resource": "directory:test/dir-a",
  "operation": "directory.readwrite"
}
```

Response:

```json
{
  "allowed": true,
  "session_id": "dsess_01HV...",
  "mode": "readwrite"
}
```

## Control-center API

The runtime API above is for `underwrapd`.

A separate control-plane API can back the operator UI / control center. It may
live in the same service with stricter authorization, or in a separate admin
surface. That is an implementation choice.

What matters is the object model.

### Suggested control-plane resources

#### 1. List instance policy

```http
GET /v1/admin/instances/{instance}/policy
```

Response:

```json
{
  "instance": "underwrap:personal:work@vm-01",
  "wrapper_states": [
    {
      "wrapper": "gh-comment",
      "enabled": true
    },
    {
      "wrapper": "aws-prod",
      "enabled": false,
      "reason": "disabled pending review"
    }
  ],
  "rules": [
    {
      "wrapper": "gh-comment",
      "resource": "secret:github/token",
      "operations": ["secret.read"],
      "effect": "allow"
    }
  ],
  "grants": [
    {
      "id": "grant-01HV...",
      "wrapper": "gh-auth",
      "resource": "libsecret:gh/default",
      "operations": ["libsecret.readwrite"],
      "effect": "allow",
      "expires_at": "2026-04-15T12:30:00Z"
    }
  ]
}
```

#### 2. Set wrapper enabled state

```http
PUT /v1/admin/instances/{instance}/wrappers/{wrapper}/state
Content-Type: application/json
```

Request:

```json
{
  "enabled": false,
  "reason": "temporary revocation"
}
```

#### 3. Upsert baseline rule

```http
PUT /v1/admin/instances/{instance}/rules/{rule_id}
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "gh-comment",
  "resource": "secret:github/token",
  "operations": ["secret.read"],
  "effect": "allow"
}
```

#### 4. Create temporary grant

```http
POST /v1/admin/instances/{instance}/grants
Content-Type: application/json
```

Request:

```json
{
  "wrapper": "aws-prod",
  "resource": "secret:aws/prod",
  "operations": ["secret.read"],
  "effect": "allow",
  "expires_at": "2026-04-15T12:30:00Z",
  "reason": "one-time deployment"
}
```

#### 5. Revoke grant

```http
DELETE /v1/admin/instances/{instance}/grants/{grant_id}
```

## Error model

Suggested error codes:

- `unauthenticated`
  - mTLS/client identity failed
- `unknown_instance`
  - no namespace mapping for authenticated principal
- `access_denied`
  - policy denied the wrapper/resource/operation
- `invalid_request`
  - malformed resource/operation/request body
- `not_found`
  - requested object does not exist
- `conflict`
  - version or state conflict if versioning is added
- `internal`
  - backend internal failure
- `backend_unavailable`
  - upstream storage unavailable if applicable

## Audit model

The backend should log at least:

- timestamp
- authenticated instance principal
- trusted wrapper name
- resource
- operation
- allow/deny decision
- result status
- optional local uid from `underwrapd` audit context
- optional admin actor for control-plane changes

Never log:

- secret material
- tokens
- libsecret values
- directory file contents

## Suggested protocol structs

If these are shared in Rust types, the core runtime request could look roughly like:

```rust
struct AuthorizedOperationRequest {
    wrapper: String,
    resource: String,
    operation: String,
    scope: Option<String>,
    audit: Option<AuditContext>,
}

struct AuditContext {
    peer_uid: Option<u32>,
}
```

And representative responses:

```rust
enum AuthorizedOperationResponse {
    ReadOk { material: Option<SecretBytes> },
    WriteOk,
    DeleteOk { existed: bool },
    ListOk { ids: Vec<String> },
    AccessDenied { message: String },
    Error { code: String, message: String },
}
```

Exact names are not important yet; the shape is.

## Test implications

This schema implies these tests must exist.

### Unit tests

- policy precedence: disabled > deny > grant allow > baseline allow > default deny
- wrapper/resource/operation tuple evaluation
- grant expiry handling
- resource kind parsing

### Integration tests

- same instance, two wrappers, different policy decisions
- same wrapper, two resources, different policy decisions
- denied request never reaches storage backend
- revoked grant takes effect for new requests

### VM tests

- central disable of wrapper blocks future invocations
- central allow of wrapper/resource enables invocation
- instance A and B remain isolated
- libsecret and directory modes still work under policy control

## Deliberate non-goals for this schema

This draft does **not** yet decide:

- whether the control plane lives in the same process/listener as the runtime API
- whether policy is stored in the same backend store as secrets or in a separate metadata store
- whether existing long-running sessions should be revoked immediately or only affect new sessions
- whether policy objects have first-class names/tags beyond what is shown here

## Recommended next step

The next design step after this document should be:

1. freeze the canonical `resource` and `operation` namespaces
2. decide whether runtime operations use generic `/read` `/write` endpoints or kind-specific endpoints
3. decide whether temporary grants are required for v1 or can follow baseline rules + wrapper enable/disable only
4. map this schema onto concrete Rust request/response types in `underwrap-protocol`
