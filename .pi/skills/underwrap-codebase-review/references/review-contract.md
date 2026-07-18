# Underwrap codebase review contract

The controller reads this file completely before launching review subagents and
passes the relevant sections to each child. This file defines evidence and
report formats; `../SKILL.md` defines authority, orchestration, drift handling,
and stop conditions.

## Current-standards research

The researcher reads the repository manifests and the security mechanism names
actually used, then establishes a dated primary-source baseline for:

- current stable Rust, the active edition, Cargo, rustdoc, Clippy, and directly
  relevant standard-library/language guidance;
- current supported React, TypeScript, Vite, Vitest, Playwright, and major
  frontend libraries present in `web/package.json`;
- current Nix, nixpkgs/NixOS module, flake, and systemd-hardening guidance;
- official Linux/kernel and protocol documentation needed for documented
  security mechanisms.

Prefer official documentation, specifications, upstream release notes, project
maintainer material, and advisories. Include source URL, relevant version/date,
and retrieval date. Do not transmit repository code or values in search
queries. Currency alone may justify a low finding; higher priority requires a
concrete security, correctness, support, or maintainability consequence.

Assign every substantive observation an id `S-001`, `S-002`, etc. Include gaps
as `S-G001`, so synthesis can account for every item.

## Ten specialist roles

Each specialist inventories and reads its assigned production files, adjacent
tests, configuration, and relevant documentation. Domain reviewers assess all
applicable angles: local simplicity, coding practice, comment/Rustdoc balance,
current standards, correctness/robustness, local design, tests, observability,
and security invariants. Cross-cutting reviewers deepen one angle across domain
boundaries.

| ID | Role | Primary scope |
| --- | --- | --- |
| R01 | Shared libraries and formats | `underwrap-protocol`, both store crates, `underwrap-keygen`, canonical formats, policy, persistence primitives |
| R02 | Backend | backend runtime/admin APIs, auth, storage coordination, embedded SPA boundary, wire/OpenAPI consistency |
| R03 | Frontend daemon | `underwrapd`, backend client, peer authentication, wrapper flow, resource channel, IPC lifecycle |
| R04 | Privileged and target chain | `underwrap-exec`, `underwrap-ns`, `underwrap-secretsd`, debug target, UID/capability/fd/namespace/projection boundaries |
| R05 | Web application | React/TypeScript/CSS, generated API use, auth/bootstrap, forms, state, accessibility-relevant implementation, tests |
| R06 | Nix and deployment | flake, NixOS module, VM harness, examples, service hardening, identities, generated configuration, operational defaults |
| R07 | Documentation quality | every published doc/example plus cross-domain comments and Rustdoc: excess, absence, staleness, duplication, and merely narrative comments |
| R08 | Architecture and design simplicity | responsibilities, protocols, dependency direction, vocabulary, config/API shape, duplication, unnecessary compatibility paths |
| R09 | Tests and validation | invariant coverage, test quality/layout, negative cases, brittle duplication, CI/check alignment, missing integration/VM/browser evidence |
| R10 | Security claims | complete claim inventory/traceability and adversarial verification of the highest-risk boundaries |

R03, R04, and R10 independently overlap on the privileged local boundary. R01,
R02, and R10 overlap on backend policy and persistence. R08 independently
challenges design conclusions from domain reviewers.

### Complete security matrix

R10 inventories every distinct normative guarantee, precondition, invariant,
mechanism statement, limitation, and non-goal in `docs/security.md`, plus the
security-critical rules in `AGENTS.md`. Exact restatements may share a row;
distinct claims may not be omitted. Assign stable `SC-001` ids and classify each:

- fulfilled;
- partially fulfilled;
- contradicted;
- not verified.

Each row cites source claim, implementation evidence, test evidence, and a
linked candidate finding or gap. “Fulfilled” means supported within documented
preconditions by reviewed evidence, not proved.

## Raw reviewer output

Every specialist uses this structure. Every substantive observation receives an
id so it cannot disappear during synthesis.

```markdown
# Review RNN: <role>

## Coverage
- Files/areas inspected: ...
- Exclusions or limits: ...
- Commands run: ...

## Confirmed strengths
### RNN-S001 — <strength>
- Evidence: `path:line-line` ...

## Candidate findings
### RNN-F001 — <title>
- Priority: critical | high | medium | low
- Confidence: high | medium | low
- Category: ...
- Location: `path:line-line`
- Evidence: ...
- Consequence: ...
- Recommendation: ...
- Behavior change: no | yes — ...
- Security boundary: none | asset; attacker; direction; invariant
- Related items: ...
- Validation: command/result or requested check
- Standards sources: primary URLs and retrieval date, or none

## Opportunities
### RNN-O001 — <intentional behavior alternative>
- Evidence and motivation: ...
- Behavior change: ...
- Trade-off: ...
- Related items: ...

## Open verification gaps
### RNN-G001 — <gap>
- Missing evidence: ...
- Best next check: ...
```

A finding requires concrete evidence and consequence. Low findings may cover
localized naming, comment quality, small duplication, mild standards drift, or
similar polish, but not unexplained personal preference. No finding is a valid
result when coverage and limits are still reported.

## Validation output

The executable validator assigns an id `V-001`, `V-002`, etc. to every command
record, including successful commands, and to every additional observation or
gap. For each command record include:

- exact command;
- exit status;
- duration;
- concise non-secret output;
- research/reviewer/security ids informed;
- skip, timeout, or unavailability reason where applicable.

It reports pre/post status and baseline-manifest comparison. Command success is
validation evidence, not proof that all related claims are fulfilled.

## Final disposition ledger

The synthesis ledger has one row for every structured input id:

- standards observations and gaps (`S-*`);
- strengths, findings, opportunities, and gaps from every reviewer (`RNN-*`);
- every validator command, discovery, and gap (`V-*`).

Allowed dispositions are:

- accepted as final finding `<F-ID>`;
- merged into final finding `<F-ID>`;
- accepted as confirmed strength;
- deferred low-impact finding `<F-ID>`;
- moved/retained as opportunity `<O-ID>`;
- unresolved/needs verification `<U-ID>`;
- contextual evidence only;
- rejected, with a reason.

Security-claim rows remain in the complete claim matrix and link to their final
finding/gap where relevant. Repetition does not prove a claim; minority feedback
is not discarded merely because other reviewers omitted it.

## Priority calibration

- **critical** — evidenced first-release blocker: direct security-claim breach,
  likely secret exposure, destructive integrity failure, or inability to build
  or use the supported product;
- **high** — material security, correctness, reliability, or architectural
  defect with broad or serious consequence;
- **medium** — meaningful maintainability, design, testing, documentation, or
  standards issue worth addressing after high-priority work;
- **low** — localized debt/polish retained for reconsideration only after
  higher-impact work and a fresh follow-up review.

## Consolidated report

The synthesis output uses this schema and contains no ticket plan.

```markdown
# Underwrap codebase review: <scope>

Date/time: <timestamp and timezone>
Mode: fresh | follow-up
Scope: ...
Baseline commit: ...
Baseline manifest checksum: ...
Working tree: clean | concise pre-existing changes
Subagent runs/artifacts: ...
Prior report: path | none

## Executive synthesis
- Counts by priority and disposition
- Security-claim status counts
- Highest-impact conclusions
- Overall confidence and material limitations

## Method and coverage
- Ten-role coverage matrix
- Standards evidence date
- Validation posture and gaps

## Confirmed strengths

## Integrated remediation themes
### C-001 — <theme>
- Findings: ...
- Cross-perspective rationale: ...
- Dependencies/order: ...
- Behavior impact: ...
- Follow-up review gate: ...

## Priority findings
### F-001 — <title>
- Priority: critical | high | medium
- Confidence: high | medium | low
- Categories/perspectives: ...
- Locations: `path:line-line`
- Evidence: ...
- Consequence: ...
- Recommendation: ...
- Behavior change: no
- Security boundary: none | asset; attacker; direction; invariant
- Related findings/theme: ...
- Validation evidence: ...
- Standards sources: ...
- Active ticket overlap: path | none

## Deferred low-impact findings
### F-0NN — <title>
<same evidence fields, concise but actionable>

## Opportunities requiring behavior approval
### O-001 — <title>
- Motivation/evidence: ...
- Intentional behavior change: ...
- Trade-offs: ...
- Related findings: ...

## Security claim matrix
| Claim ID | Type | Source | Claim | Assessment | Implementation evidence | Test evidence | Finding/gap |

## Current-standards assessment
- Dated conclusions with primary-source links

## Validation results
| Command | Result | Duration | Evidence/claims | Notes |

## Active ticket overlap

## Follow-up comparison
<resolved/persistent/superseded/not-reverified/new, or “not applicable”>

## Unresolved and rejected items
### U-001 — <title>
- Source item: ...
- Reason/evidence needed: ...

## Input disposition ledger
| Raw item | Final disposition | Reason |

## Review limitations
```

Recommendations may identify the smallest coherent correction and relationships
needed for later reasoning. They must not include ticket filenames, ticket
hierarchy, acceptance criteria for future tickets, migration history, or an
implementation plan.
