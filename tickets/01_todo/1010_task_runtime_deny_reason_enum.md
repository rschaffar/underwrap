# Task: Replace stringly-typed runtime Deny reasons with a shared enum

## Problem

The runtime API's denial reasons are bare string literals scattered
across the policy evaluator and the invocation handler, and the
documented set has already drifted from the emitted set.

Emitters:

- `policy::decide` (`crates/shared-protocol/src/policy.rs:170`) emits
  `"locked"`, `"expired"`, `"not-unsafe-debug"`, and a verbatim
  operator-supplied reason for `Locked`.
- `resolve_invocation_or_deny`
  (`crates/backend-backendd/src/runtime_api/invocations.rs:84`) emits
  `"unregistered-pair"` and `"no-permission"`.
- finalize adds `"not-writable"` and the `InvalidBlob` reasons
  `"decode-failed"`, `"schema-unknown"`, `"noncanonical-blob"`,
  `"env-section-immutable"`.

The doc comment on `RuntimeWrapperInvocationOpenResponse::Deny`
(`crates/shared-protocol/src/service.rs:689`) claims the reason is "a
stable kebab-case discriminator the daemon can switch on" and lists
`no-permission, locked, expired, unregistered-pair` — it **misses**
`not-unsafe-debug` and the operator-custom reasons, and **no consumer
actually switches on it**: every daemon-side consumer
(`wrapper_flow/read.rs:124`,
`wrapper_resource_channel.rs:195,252`) just forwards the string into a
wrapper-facing error message.

So today the "discriminator" contract is aspirational and the docs are
inaccurate.

## Decision

Introduce shared reason enums so the contract is real and cannot drift.

## What to do

1. In `shared-protocol`, define enums for the runtime denial/blob
   reasons, e.g.:
   - `RuntimeDenyReason` covering `Locked` (with optional operator
     reason), `Expired`, `NotUnsafeDebug`, `NoPermission`,
     `UnregisteredPair`, `NotWritable`.
   - `RuntimeInvalidBlobReason` covering `DecodeFailed`,
     `SchemaUnknown`, `NoncanonicalBlob`, `EnvSectionImmutable`.
   Serialize kebab-case so the wire bytes are unchanged from today's
   literals (snapshot the current strings and assert equality in a
   test). Decide how to carry the `Locked` operator-custom reason
   (e.g. a `Locked { reason: Option<String> }` variant) so it survives
   the enum.
2. Change `Deny.reason` / `InvalidBlob.reason` to the enum types (or
   keep them `String` on the wire but produce them only via the enum's
   `Display`/serde — pick whichever keeps the wire byte-identical and
   the source single-sourced).
3. Update `policy::decide`, `resolve_invocation_or_deny`, and the
   finalize handler to construct the enum variants instead of literals.
4. Fix the doc comments on the `Deny` variants to list the complete,
   accurate set, and either make the "daemon can switch on it" claim
   true (daemon matches the enum) or soften it to "human-facing reason"
   if the daemon still only forwards it. Prefer matching the enum where
   it lets the daemon give a better wrapper-facing message.
5. While here: consider giving `ServiceErrorResponse.code` the same
   treatment for the runtime-relevant codes (notably `store_locked`,
   currently dispatched by a hand-compared string literal in
   `backend_client.rs:304`). Single-source that constant at minimum so
   backend and client can't drift. (Scope this conservatively — the
   primary deliverable is the Deny/InvalidBlob enums.)

## Acceptance

- `cargo test --workspace` (add a test pinning each enum variant's
  kebab-case wire string so a rename can't silently change the wire).
- `cargo clippy --all-targets --workspace -- -D warnings`
- Doc comments on the runtime Deny/InvalidBlob types match the emitted
  set exactly.
