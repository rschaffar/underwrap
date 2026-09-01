# ADR-0004: Separate internal-error responses from trusted-host diagnostics

Status: Accepted

Date: 2026-08-30

## Decision

Return one fixed value-free public message for generic admin and runtime internal errors while preserving their HTTP status and stable error code.

Generic internal-error events record a static operation context and concrete Rust error type without formatting an arbitrary source chain.

Retain source detail only for the audited admin filesystem, storage, canary, and registry failure classes whose current values are trusted-host diagnostics rather than brokered resource material.

Apply the generic rule consistently to both authenticated admin requests and mTLS-authenticated runtime requests.

[`docs/security.md`](../security.md) owns the diagnostic security boundary, [`docs/backend.md`](../backend.md) owns the API behavior, and [`docs/testing.md`](../testing.md) owns the current evidence and nonclaims.

## Alternatives considered

Returning internal source detail to clients would disclose trusted-host paths or malformed stored content across the HTTP boundary.

Formatting arbitrary generic source chains into host logs could copy attacker-influenced stored strings into diagnostics.

Applying the rule only to the admin API would leave the equivalent runtime response and logging boundary inconsistent.

Removing every audited source detail would reduce operator diagnostics for bounded failure classes without improving the public response boundary.

## Consequences

Clients receive stable value-free internal errors and cannot use a generic error path to recover host source detail.

Trusted-host operators retain bounded detail for the specifically audited source classes.

Adding or changing a detailed source class requires a fresh material-flow audit and synchronized security and test evidence.

No error event may include brokered values, resource blobs, credentials, key material, password verifiers, or certificate bodies.
