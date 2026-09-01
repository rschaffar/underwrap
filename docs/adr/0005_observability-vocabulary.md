# ADR-0005: Use one first-release observability vocabulary

Status: Accepted

Date: 2026-08-30

## Decision

Use the current wrapper and resource vocabulary directly in daemon audit events, namespace writeback diagnostics, tests, and documentation without compatibility aliases for superseded names.

The daemon exposes exactly the seven audit-helper events documented in [`docs/security.md`](../security.md), with allowed reads and resource-channel lifecycle events using the `resource` identifier field.

Namespace writeback rejection uses permission and writable terminology, including the `permission_readonly`, `server_rejected`, and `resource_too_large` reasons.

The daemon keeps its default human-readable stdout formatter, `RUST_LOG` controls event filtering, and native namespace journald records remain a separate output path.

[`docs/architecture.md`](../architecture.md) owns the component flow, [`docs/security.md`](../security.md) owns the event and field contract, and [`docs/testing.md`](../testing.md) owns the formatter and deployed evidence limits.

## Alternatives considered

Retaining legacy event, field, or reason aliases would create two first-release schemas without a deployed compatibility requirement.

Calling the default daemon stream a JSON audit log or persistent journald sink would overstate the configured formatter and service wiring.

Combining daemon tracing with namespace-native journald output would hide their distinct transport and fallback behavior.

## Consequences

Operators and tests use one current event vocabulary and can distinguish daemon tracing from namespace-native writeback records.

The seven helper events are an observability schema, not a persistent audit ledger or configurable audit sink.

Future event, field, reason, level, formatter, or sink changes are operational interface changes and require synchronized documentation and deployed evidence.
