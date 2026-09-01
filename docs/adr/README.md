# Architecture decision records

`docs/adr/` holds accepted decisions and their trade-offs.

An ADR is the maintained owner for a decision that affects architecture, security boundaries, interfaces, operations, or workflow.

Record the chosen behavior, rejected alternatives, consequences, and evidence needed to revisit the decision.

Update or supersede the ADR when the decision changes instead of hiding the change in an active plan.

## Accepted decisions

- [`ADR-0001`](0001_plans-as-active-workflow.md) — use plans as the active work workflow;
- [`ADR-0002`](0002_privileged-configuration-grammar.md) — use a strict privileged configuration grammar;
- [`ADR-0003`](0003_namespace-local-protocol-profile.md) — isolate the namespace wrapper's local protocol profile;
- [`ADR-0004`](0004_internal-error-redaction.md) — separate internal-error responses from trusted-host diagnostics;
- [`ADR-0005`](0005_observability-vocabulary.md) — use one first-release observability vocabulary;
- [`ADR-0006`](0006_rust-support-and-resolver.md) — pin the Rust support matrix and retain resolver 2;
- [`ADR-0007`](0007_frontend-tool-matrix.md) — validate one locked frontend and OpenAPI tool matrix;
- [`ADR-0008`](0008_spa-unit-coverage-gate.md) — gate the selected SPA API/library unit boundary at 80 percent.
