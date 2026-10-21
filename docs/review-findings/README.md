# Review findings

This register owns actionable review findings, their decisions and their verification evidence.
It is not a security certification, an implementation mandate or the authority for current product behavior.
Current behavior and enduring limitations belong in the maintained [topic documentation](../../AGENTS.md#start-with-the-current-owners).

## Layout and identifiers

```text
review-findings/
  README.md
  active/RF-0004/
    README.md
    finding.md
    evidence.md
  archived/RF-0001/
    README.md
    finding.md
    evidence.md
```

Each finding has exactly these three Markdown files, whether active or archived.
The empty archive may contain `.gitkeep`; create its first finding directory when a finding is actually resolved.
Allocate the next unused project-wide `RF-` number, with at least four digits, after checking both active and archived records.
Never restart numbering for a new review or reuse a removed or merged ID.
Preserve reviewer-local IDs as aliases scoped to their review baseline, not as globally unique identifiers.
Search existing records before creating a duplicate; cross-link related findings without merging distinct consequences merely because one change might fix both.

| File | Reader and required content |
| --- | --- |
| `README.md` | Human decider: title, priority, state, aliases and confidence; a brief explanation of impact, suggested fix, decision needed and material caveat. Keep the narrative around 150 words or less. This file owns current metadata and disposition. |
| `finding.md` | Implementer: trigger, affected flow and exact source locations, consequence, boundaries and non-goals, correction suggestions and alternatives, acceptance criteria, and the single work-owner link when work is approved. Security findings identify asset, attacker, boundary direction and invariant. |
| `evidence.md` | Verifier: reviewed revision and environment, source observations, actual checks and controls, a reusable reproduction or verification procedure, evidence limits, and later resolution verification. Distinguish recorded executions from proposed checks. |

The index is a navigation summary of the records, not a second authority for their status.
Update it with each record's metadata change or move.
Update the human summary when technical conclusions change; do not repeat full technical explanations or logs across all three files.

## Recording and authority

Use this register by default for actionable findings from project reviews, including explicitly labelled candidates worth verifying.
Do not turn every transient review comment, rejected hypothesis or optional redesign into a finding.
Preserve substantive contextual conclusions in their appropriate review or topic owner rather than silently dropping them during consolidation.
Recording or updating finding documents is an authorized review deliverable under this convention, subject to the [Git lifecycle](../../AGENTS.md#git-lifecycle).
Review otherwise remains source-read-only: records and suggested fixes do not authorize source changes, altered product behavior, integration, publication or cleanup.
An explicitly no-write review overrides this default; keep its output private and external and report the pending import instead.
Keep raw working reports, traces and intermediate artifacts private and external unless separately requested.

Follow [vulnerability reporting](../../SECURITY.md) for security-sensitive findings.
A local finding record is not permission to expose exploit details in a public issue, pull request or remote publication.
Never store live credentials, resource contents, tokens, keys, cookies, secret-bearing fixtures or unreviewed diagnostic captures here.
Use synthetic-data procedures and safe decision metadata; operator-selected identifiers may also be sensitive.

## States and dispositions

| Active state | Meaning |
| --- | --- |
| `needs-verification` | A concrete candidate with insufficient evidence for the claimed defect or impact; keep the missing check explicit. |
| `open` | An accepted finding awaiting a correction decision; acceptance of a finding is not acceptance of its risk. |
| `in-progress` | Work has been explicitly authorized and its scope and work owner are recorded. |
| `blocked` | An unresolved finding with a named obstacle and next decision or evidence requirement. |

Priority describes impact; confidence describes evidence strength; state describes work progress.
A source-confirmed finding can be `open` while a packaged reproduction remains outstanding.
Do not silently upgrade confidence or impact when moving states, running aggregate suites or changing a summary.

An archived record has state `archived` and one explicit resolution:

| Resolution | Required basis |
| --- | --- |
| `fixed` | Completed correction, relevant acceptance checks, the fixed revision, and any remaining limits. |
| `risk-accepted` | Explicit maintainer decision, rationale, remaining consequence and a revisit condition where applicable. Keep accepted risks visible in the index. |
| `not-a-defect` | Evidence disproving the finding or establishing the intended behavior, without erasing the original observation. |
| `duplicate` | A link to the surviving finding and confirmation that its distinct consequences and evidence were preserved. |
| `obsolete` | The change that removed applicability and evidence that the affected behavior is no longer present. |

Working on a finding, merging a partial patch or lacking a required check is not a `fixed` disposition.
An agent must not accept risk on the maintainer's behalf.
Before archiving, update all three files as needed, record the concise disposition and verification basis, and move the whole folder from `active/` to `archived/`.
Update the index and affected cross-links in the same change.
Reopen under the same ID, retaining prior evidence and explaining why it no longer settles the finding.
An archive is a revision-scoped record, not a claim that its original source locations describe today's code.

## Planning and completion

Use the [planning skill](../../.agents/skills/writing-plans/SKILL.md) and one explicit work owner.
A finding's correction suggestions and acceptance criteria are not an approved execution plan.
For a small authorized fix, `finding.md` may own a short scope/evidence checklist with intended behavior, non-goals and execution authority.
For a larger or shared correction, link one agreed plan or issue from every affected finding instead of duplicating plans.
A shared plan does not collapse separate findings' acceptance criteria or permit closing them all after one passing check.
Record a blocker where progress stops; reopen evidence obligations when relevant code or boundaries change.

After a correction, put enduring behavior and limitations in their maintained topic owner, rationale beside the mechanism and regression evidence in tests.
The finding retains only the problem, decision and necessary evidence—not approval transcripts, an execution diary or a second copy of the product manual.
Follow the [testing matrix](../testing.md#choose-checks-by-change) for the affected boundary.

## Evidence and maintenance checks

Make essential evidence usable without temporary directories or private machine paths.
Retain exact repository paths and symbols, the reviewed revision, relevant inputs described without values, observed results and meaningful controls.
For each executed check, record its command, exit and duration when known, environment prerequisites and the property actually observed.
If a command or probe is unavailable, say so; a normalized rerun recipe is not a retained original executable or a new successful execution.
Source inspection, simulated witnesses, packaged execution and user-reported observations remain distinct.
Keep discovery evidence separate from correction verification, and never convert a suite pass into proof of an untested trigger, operational custody or security certification.

Before committing register changes, check:

- Unique IDs and aliases scoped to a review; exactly three files per finding; index and metadata agreement.
- Valid local links and paths, including cross-links after a move, and no required dependency on temporary artifacts.
- Brief decision summaries, complete evidence qualifications and preservation of meaningful merged consequences.
- No secret material, unreviewed captures, accidental public disclosure or unsupported claims of execution or resolution.
- Compatibility with the planning owner and explicit authority; documentation changes alone do not require rerunning unrelated runtime suites.
- Syntax and behavior of any newly introduced executable examples or tooling under the testing matrix; no checks are waived by storing a command in Markdown.

## Review provenance and index

The initial entries derive from the comprehensive independent review of `01214454aee850ebe8ceef5b757b9ae61cca07aa` on 2026-10-08.
They retain 37 accepted findings and one unresolved candidate, not 38 confirmed production vulnerabilities.
Their original priorities are one High, one conditional High, 14 Medium and 21 Low, with a separate Low candidate.
The review's independent synthesis acceptance covers its evidence and accounting, not project security or release readiness.
The import does not apply any correction or rerun the recorded behavioral checks.
Reviewer-local aliases are preserved below, including A-01/S-02/W-01 merged into F-01 and A-03 merged into F-02.

The previous review also distinguishes intentional semantics and contextual observations from findings.
In particular, trusted-target disclosure, whole-blob last-writer-wins, non-draining rotation, incomplete erasure and operational custody assumptions are not new defects.
The CSRF lifetime comment versus six-hour dependency default is contextual to RF-0036; collision-resistant rather than collision-proof ID wording is contextual to RF-0003.
Optional resource-admission/bookkeeping bounds remain availability alternatives, not demonstrated vulnerabilities or implementation commitments.
The register is an actionable-findings migration, not an archive of the full review's claim inventory or every working report.

| ID | Review aliases | Priority | State | Finding |
| --- | --- | --- | --- | --- |
| [RF-0001](archived/RF-0001/README.md) | `N-01` | High | `archived` (`fixed`) | Replaceable staging root can substitute collected files |
| [RF-0002](archived/RF-0002/README.md) | `B-01` | High (conditional) | `archived` (`fixed`) | Dynamic-debug SPA fallback can disclose an outside symlink target |
| [RF-0003](archived/RF-0003/README.md) | `F-01`, `A-01`, `S-02`, `W-01` | Medium | `archived` (`fixed`) | Structured admin libsecret identifiers fail initial publication |
| [RF-0004](archived/RF-0004/README.md) | `B-02` | Medium | `archived` (`fixed`) | Authenticated activity does not renew inactivity expiry |
| [RF-0005](archived/RF-0005/README.md) | `L-01` | Medium | `archived` (`fixed`) | Manual store-cleanup command does not quote its path |
| [RF-0006](active/RF-0006/README.md) | `S-01` | Medium | `in-progress` | Accepted alias changes leave exported D-Bus routing stale |
| [RF-0007](active/RF-0007/README.md) | `S-03` | Medium | `in-progress` | Deleted D-Bus objects acknowledge setters without changing state |
| [RF-0008](active/RF-0008/README.md) | `W-02` | Medium | `open` | Accepted dot-only libsecret IDs normalize out of request paths |
| [RF-0009](archived/RF-0009/README.md) | `W-03` | Medium | `archived` (`fixed`) | Text reveal and copy drop a leading UTF-8 BOM |
| [RF-0010](active/RF-0010/README.md) | `W-05` | Medium | `open` | An old form submission can close a reopened draft |
| [RF-0011](active/RF-0011/README.md) | `N-02` | Medium | `open` | Failed usage reporting hides a known writeback rejection |
| [RF-0012](active/RF-0012/README.md) | `X-01` | Medium | `blocked` | Runtime-base paths are not escaped for tmpfiles syntax |
| [RF-0013](active/RF-0013/README.md) | `X-02` | Medium | `open` | Instance selectors collide with internal generated names |
| [RF-0014](archived/RF-0014/README.md) | `T-01` | Medium | `archived` (`fixed`) | Packaged-process tests lack failure-path kill and reap ownership |
| [RF-0015](archived/RF-0015/README.md) | `T-02` | Medium | `archived` (`fixed`) | Parallel fixtures use non-reentrant group lookup storage |
| [RF-0016](active/RF-0016/README.md) | `T-03` | Medium | `open` | Malformed input cannot distinguish HTTP body-limit enforcement |
| [RF-0017](active/RF-0017/README.md) | `F-02`, `A-03` | Low | `open` | Canonical-size expansion is reported as an internal finalize failure |
| [RF-0018](active/RF-0018/README.md) | `A-02` | Low | `open` | Permission-cleanup diagnostics mislabel unavailable store states |
| [RF-0019](active/RF-0019/README.md) | `D-01` | Low | `open` | A held peer pidfd does not pin numeric procfs lookup |
| [RF-0020](archived/RF-0020/README.md) | `L-03` | Low | `archived` (`fixed`) | Cleanup dialog says YAML was removed instead of a registry record |
| [RF-0021](active/RF-0021/README.md) | `N-03` | Low | `open` | Post-Armed wording omits the initialized-generation baseline |
| [RF-0022](active/RF-0022/README.md) | `X-03` | Low | `open` | Selected proc probes confuse access denial with other errors |
| [RF-0023](active/RF-0023/README.md) | `X-04` | Low | `open` | Per-crate builds and integration runner omit --locked |
| [RF-0024](active/RF-0024/README.md) | `X-05` | Low | `open` | Fixture comment describes obsolete executable-keyed identity |
| [RF-0025](active/RF-0025/README.md) | `T-04` | Low | `open` | Store-parameterized permission helpers seed the default store |
| [RF-0026](archived/RF-0026/README.md) | `T-05` | Low | `archived` (`fixed`) | Readiness loops can wait about 26 minutes rather than 60 seconds |
| [RF-0027](active/RF-0027/README.md) | `T-06` | Low | `open` | UsageSignal timestamp tests cannot distinguish finalize from open |
| [RF-0028](active/RF-0028/README.md) | `T-07` | Low | `open` | Body-impersonation test sends no competing body identity |
| [RF-0029](active/RF-0029/README.md) | `T-08` | Low | `open` | Failing equality assertions can print unwrapped fixture material |
| [RF-0030](active/RF-0030/README.md) | `W-04` | Low | `open` | Migration retry policy conflicts with authoritative-reconciliation guidance |
| [RF-0031](active/RF-0031/README.md) | `W-06` | Low | `open` | Pair resource controls lack their intended programmatic labels |
| [RF-0032](active/RF-0032/README.md) | `W-07` | Low | `open` | Duplicate resource rename is misreported as a network error |
| [RF-0033](active/RF-0033/README.md) | `W-08` | Low | `open` | Locked-store delete guidance claims contents were inspected |
| [RF-0034](active/RF-0034/README.md) | `W-09` | Low | `open` | Rotation dialog understates shared lifecycle serialization |
| [RF-0035](active/RF-0035/README.md) | `C-01` | Low | `open` | PATH-dependent keygen instructions can select an entry wrapper |
| [RF-0036](active/RF-0036/README.md) | `C-02` | Low | `open` | Security describes the wrong CSRF token-binding mechanism |
| [RF-0037](active/RF-0037/README.md) | `M-01` | Low | `open` | Dev-only source-map-js lock selection has a patched advisory |
| [RF-0038](active/RF-0038/README.md) | `L-02` | Low (candidate) | `needs-verification` | Compound workspace-reservation failure may leave inconsistent recovery state |
