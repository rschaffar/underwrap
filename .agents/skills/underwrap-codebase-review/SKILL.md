---
name: underwrap-codebase-review
description: Review the whole repository or an explicitly comprehensive subsystem with independent evidence and verification.
---

# Comprehensive review

Resolve the requested scope, purpose, depth and working-tree baseline before starting.
Read [agent guidance](../../../AGENTS.md), [Architecture](../../../docs/architecture.md), [Security](../../../docs/security.md), [Testing](../../../docs/testing.md), and the [review contract](references/review-contract.md).
This is an engineering assessment, not a formal proof or an independent security certification.

## Authority

Review only: do not fix source, generate tracked-contract changes, execute correction plans, or rewrite history.
Recording actionable findings follows the [project convention](../../../docs/review-findings/README.md) and applicable Git lifecycle; it does not authorize their implementation, integration or publication.
Keep raw reports and intermediate evidence in a private directory outside the repository.
An explicitly no-write review keeps all output external and reports any pending register import.
Do not quote credentials, brokered values, keys or secret-bearing fixtures.
Review the current working tree, including relevant existing changes; never clean it to make review convenient.

## Scope and baseline

Inventory in-scope files, current declarations and relevant work ownership without treating historical approval as current truth.
Use CodeGraph before structural discovery when an index exists.
Hash the scope with `scripts/scope-manifest.py`; record the baseline and compare it after research, review, executable validation and before reporting.
On drift, stop and re-establish a coherent baseline rather than publishing mixed-revision evidence.
The script records hashes and metadata, not file contents.
Complete the unchanged-baseline review and synthesis before importing its findings into the register.
Treat that import as a separate documentation-only phase: record its paths and verify that reviewed source and contracts remain unchanged instead of calling the new full-tree manifest identical.

Choose independent review angles that cover the requested scope without a mandatory fixed-size panel.
For a broad code review, consider shared formats/storage, backend, daemon, privileged chain/Secret Service, SPA, Nix/deployment, documentation, architecture, tests and security claims.
For documentation-only work, focus on reader tasks, structure, prose, source accuracy and preservation of current decisions.
Use multi-agent orchestration only with explicit user opt-in; state cost/coverage trade-offs and do not silently expand the scope.

## Execution

Give each reviewer exact scope, evidence requirements, exclusions, a fresh context and an external output path.
Read assigned files completely; account for both line and byte truncation and partition oversized files explicitly.
Do not accept a reviewer that reports incomplete coverage as complete.
An unavailable angle requires a fresh replacement or the user's explicit acceptance of a limited review before synthesis.
Do not repeat a failed full run when a bounded replacement can close the actual gap.

Use primary-source standards research only where it informs the task; currency alone is not a serious defect.
Coordinate executable checks so expensive suites run once and cannot silently rewrite source.
Run checks relevant to the reviewed boundary, label skipped/unavailable results, and preserve any unexpected mutation without reverting user work.
Independently verify consequential or disputed findings, then synthesize without erasing distinct low-impact observations or turning preferences into defects.
Have a separate reviewer check the synthesis for evidence, omissions, overclaims and scope integrity when independent review is part of the agreed method.
If verification fails, report the exact blocker rather than presenting the draft as accepted.

## Handoff

Present the verified conclusions, complete finding index, intentional alternatives, checks and meaningful gaps, with the report paths and reviewed baseline.
For register imports, follow the project convention's three-file structure, stable IDs and review-scoped aliases; retain the final verified dispositions, deduplicated consequences and evidence limits.
Make each record's essential evidence durable without copying raw reports, temporary paths or the full claim inventory into the register.
Keep the full review's contextual conclusions and claim inventory in its review report; do not manufacture extra findings to archive every observation.
Verify the import's substantive fidelity and links separately from the review's original behavioral checks.
Stop before implementing corrections; applying recommendations requires a separate user request.
