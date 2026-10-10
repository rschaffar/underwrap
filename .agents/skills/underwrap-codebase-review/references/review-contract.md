# Review evidence contract

Use this contract to make a review reproducible and useful, not to generate an execution diary.
The requested scope determines the necessary reviewers, standards research, checks, claim inventory and report length.

## Findings and alternatives

A finding requires a concrete trigger or mismatch, current evidence, a consequence, and a bounded correction.
Include priority, confidence, exact source location, checks run or requested, and whether the recommendation preserves intended behavior.
Security findings also identify the protected asset, attacker, direction, and violated documented invariant.
Use critical/high for evidenced severe or blocking defects, medium for meaningful bounded corrections, and low for localized debt or polish.
Do not manufacture findings to fill categories.
Keep optional product or workflow alternatives separate, with their motivation and trade-offs rather than implied implementation authority.

## Reviewer output

Each reviewer states complete files or explicitly assigned ranges read, source/test exclusions, commands, confirmed strengths, candidate findings, alternatives and verification gaps.
Use stable report-local IDs when several reviewers need deduplication and accounting.
When importing actionable findings into the [project register](../../../../docs/review-findings/README.md), assign stable project IDs and preserve those report-local IDs as review-scoped aliases.
Keep the final verified severity, confidence, distinct merged consequences and missing evidence; a migration is not fresh behavioral verification.
A file request is not read evidence if its returned content was truncated.
Existing tests are leads: inspect important assertions and distinguish that inspection from a current passing execution.
Do not quote secret-bearing inputs or fixtures.

## Executable evidence

For each relevant command record its exact invocation, exit, duration, safe summary, related findings and unavailable/timeout reason.
A successful unit test does not establish kernel behavior, operator custody, filesystem durability, comprehensive zeroization or target trustworthiness.
Do not run a tracked-file generator under a read-only review merely because it is named a test.
Compare the baseline before and after potentially mutating validation and preserve unexpected state without automatic rollback.

## Security claims

When security claims are in scope, inventory distinct guarantees, preconditions, mechanisms, limitations and non-goals from their normative owners.
Exact restatements may share a row; distinct claims must not disappear in a summary.
For each, cite the claim, current source, actual test evidence and any finding or gap.
Classify support as fulfilled within stated scope, partial, contradicted or not verified; these labels are not a security score.
Link every partial, contradicted or unverified claim to the evidence limitation or corrective finding.
Never turn a trusted-target assumption into an adversarial-target sandbox claim.

## Synthesis and verification

Deduplicate repeated findings without losing distinct consequences, minority evidence, meaningful low-impact observations or rejected hypotheses.
Verify serious/disputed findings against the actual current source or executable result.
For cross-review synthesis, account for every substantive input with an explicit accepted, merged, deferred, alternative, unresolved, contextual or rejected disposition.
Mechanical ID completeness is not substantive preservation.
When removal/consolidation is requested, map each important current decision, limitation and outstanding obligation to a surviving owner; discard chronology, not meaning.
Do not revive completed work or convert an optional direction into a commitment.

A report contains an executive answer, baseline/scope/method, strengths, prioritized findings, alternatives, validation results, meaningful unresolved/rejected items and limitations.
Add claim matrices, curation maps and input ledgers only where the agreed scope needs them.
An independent report check verifies content and important omissions, not just headings or counts.
State a failed acceptance gate honestly and obtain replacement evidence or explicit limited-scope approval before claiming completion.
Do not embed a new implementation plan or imply that review authorizes fixes, publication or integration.
Register records may contain bounded correction suggestions and acceptance criteria, but execution planning and later archival follow the single-owner and disposition rules of the project convention.
