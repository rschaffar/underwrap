# ADR-0003: Isolate the namespace wrapper's local protocol profile

Status: Accepted

Date: 2026-08-27

## Decision

Keep `underwrap-protocol`'s default `full` feature profile for ordinary consumers and provide an explicit `local-abi` profile for `underwrap-ns`.

`underwrap-ns` disables protocol default features and selects only `local-abi`.

The local profile retains the canonical resource, execution, publication, target-environment, message, and synchronous local ABI needed by the namespace wrapper.

Configuration, policy, service, schema, asynchronous IPC, and SCM_RIGHTS surfaces remain in the default full profile.

[`docs/architecture.md`](../architecture.md) owns the current module boundary, and [`docs/testing.md`](../testing.md) owns the isolated package checks and their limits.

## Alternatives considered

Leaving every protocol module in the namespace package's compile graph would preserve unrelated source and dependency coupling in a world-executable privileged component.

Splitting a new crate would create a larger package and API migration than the current local boundary requires.

Claiming workspace-wide isolation would be incorrect because Cargo may unify protocol features when a workspace build also contains a full-profile consumer.

## Consequences

An isolated `underwrap-ns` package build excludes the full-only protocol modules and their protocol-owned Tokio, Utoipa, Serde YAML, and libc edges while retaining `underwrap-ns`'s direct libc dependency.

This narrows isolated compile and review coupling only.

It does not establish a smaller linked binary, runtime trusted computing base, attack surface, exploitability set, or vulnerability set.

New local ABI needs must be assigned deliberately to the always-compiled boundary or justified as a full-profile dependency.
