---
name: underwrap-codebase-review
description: Review the whole repository or an explicitly comprehensive subsystem with independent evidence and verification.
---

# Comprehensive review

Resolve the requested scope, purpose, depth and working-tree baseline before starting.
Read [agent guidance](../../../AGENTS.md), [Architecture](../../../docs/architecture.md), [Security](../../../docs/security.md), [Testing](../../../docs/testing.md), and the [review contract](references/review-contract.md).
This is an engineering assessment, not a formal proof or an independent security certification.

## Authority

Review only: do not fix source, generate tracked-contract changes, create implementation work, commit, or rewrite history.
Keep reports and intermediate evidence in a private directory outside the repository unless the user explicitly requests another destination through the applicable Git lifecycle.
Do not quote credentials, brokered values, keys or secret-bearing fixtures.
Review the current working tree, including relevant existing changes; never clean it to make review convenient.

## Scope and baseline

Inventory in-scope files, current declarations and relevant work ownership without treating historical approval as current truth.
Use CodeGraph before structural discovery when an index exists.
Hash the scope with `scripts/scope-manifest.py`; record the baseline and compare it after research, review, executable validation and before reporting.
On drift, stop and re-establish a coherent baseline rather than publishing mixed-revision evidence.
The script records hashes and metadata, not file contents.

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

Present the verified conclusions, complete finding index, intentional alternatives, checks and meaningful gaps, with the external report path and baseline.
Keep detailed claim/evidence inventories in the report rather than creating permanent repository bureaucracy.
Stop before implementation; applying recommendations requires a separate user request.
