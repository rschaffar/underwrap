---
name: underwrap-codebase-review
description: Comprehensive multi-agent review of the underwrap repository or an explicitly scoped subsystem. Use for evidence-backed assessment of code and design simplicity, coding practices, documentation quality, current stable Rust/web/Nix standards, tests, and whether documented security claims are fulfilled. Produces one synthesized report and stops before plan creation or implementation.
compatibility: Intended for the underwrap repository layout. Requires pi-subagents and Python 3. The only project write is one new consolidated report under the append-only tickets/_review/ directory.
---

# Underwrap codebase review

Run an independent, multi-angle review of the current working tree. The whole
repository is the default scope; use a narrower scope only when the user
explicitly requests one.

Read [the review contract](references/review-contract.md) completely before
launching subagents. It defines the ten specialist roles, evidence formats,
security matrix, priorities, synthesis ledger, and final report schema.

## Authority and stop boundary

All substantive review is performed by subagents. Ten specialist subagents
produce independent evidence, and a separate synthesis subagent verifies and
combines it. A final report-validator subagent checks the synthesis. The
controller may gather mechanical context, orchestrate, compare manifests, copy
the accepted synthesis byte-for-byte, and present it; the controller must not
replace failed synthesis with its own analysis.

During this skill:

- do not edit production code, tests, configuration, examples, or published
  documentation;
- do not create, edit, move, number, or propose epics, stories, or tasks;
- do not apply fixes;
- do not clean, stash, restore, overwrite, or commit working-tree changes;
- do not log or quote secret values, credentials, resource blobs, key material,
  tokens, or secret-bearing fixture contents.

The only project write is one new final report under the append-only
`tickets/_review/` directory. Never overwrite or edit an existing report.
Configured subagent artifacts and baseline manifests live outside the repository.
Commands may create ignored build/test outputs, but other tracked-file changes
are forbidden.

Plan derivation happens only after the controller presents the complete review
index and the user makes a later explicit request. That request uses
`writing-plans` and must revalidate the report against the current tree.

## Review posture

Read and follow `AGENTS.md`, especially its greenfield posture and threat model.
Read `docs/security.md` before assessing security.

- Treat current functionality as intentional unless repository evidence shows a
  correctness, robustness, or security defect.
- Put preserve-behavior improvements in Findings.
- Put alternative product behavior, workflows, or capabilities only in
  Opportunities, separate from defects.
- Prefer the clean first-release design over compatibility with an undeployed
  earlier shape.
- Do not manufacture findings to fill categories.
- Preserve every evidence-backed minor observation in the deferred low-impact
  section. It must not distract from high-impact work or silently disappear.
- Relate findings from different perspectives in remediation themes without
  turning those themes into plans or implementation work.

Every security concern names the protected asset, attacker, boundary direction,
documented claim/invariant, and implementation/test evidence. This remains an
engineering assessment, not a formal proof or claim of independent audit.

## Review modes

### Fresh

This is the default. Specialist reviewers receive no prior review report and no
active plan inventory and historical ticket overlap.

### Follow-up

Perform the same fresh independent review first. Only the synthesis subagent
receives the prior report, then classifies prior findings as resolved,
persistent, superseded, not reverified, or replaced by a newly framed finding.
Use the latest report with the same scope unless multiple candidates are
materially ambiguous; then ask the user which report to compare.

## Preflight

The controller performs only these mechanical steps:

1. Record the timestamp/timezone, `git rev-parse HEAD`, `git status --short`, and
   `git diff --cached --name-only` without changing the index.
2. Resolve scope. Whole-repository scope includes Rust crates, React/TypeScript
   and CSS, Nix/module/VM code, integration tests, examples, build/tooling
   configuration, generated API contracts, and all published documentation.
   Exclude `.git/`, ignored build outputs, dependency/vendor directories, and
   result symlinks.
3. Read `AGENTS.md`, `docs/index.md`, `docs/architecture.md`,
   `docs/security.md`, and `docs/testing.md`, plus other scope-relevant docs.
4. Inventory pinned toolchains/dependencies. Treat committed generated OpenAPI
   and TypeScript schema files as contract evidence.
5. Scan `docs/plans/` for active-plan overlap and the retained ticket directories
   for historical work that could constrain migration, but withhold that
   inventory from research and specialist reviewers to avoid anchoring.
6. Resolve the prior report in follow-up mode, but withhold it from research and
   specialist reviewers.
7. Select a non-existing report path:
   `tickets/_review/codebase_<YYYY-MM-DD>.md`, or
   `tickets/_review/<scope_slug>_<YYYY-MM-DD>.md` for explicit scope. Add a
   numeric suffix rather than overwriting.
8. Call `subagent({ action: "list" })` and use only executable, non-disabled
   agents. Prefer `researcher`, `reviewer`, and `context-builder`.
9. Create a private artifact directory outside the repository and record it for
   the report. Give every child a unique output path.
10. Tell the user the scope, mode, current dirty-tree state, fanout, validation
    posture, and intended report path in one concise update.

The current working tree—including relevant uncommitted and untracked files—is
the review baseline. Do not require a clean tree and do not substitute `HEAD`.

## Stable-baseline guard

Use `scripts/scope-manifest.py` to hash the path, type, mode, and content of
every in-scope regular file or symlink without recording file contents. The
script rejects unsupported special entries rather than claiming incomplete
coverage. Save the JSON and its checksum in the private artifact directory.
Generate it twice before launch and require identical results.

Regenerate and compare the manifest:

- after standards research;
- after specialist fanout and any retry;
- before and after executable validation;
- before synthesis;
- before publication.

If it changes, the stage may have reviewed mixed revisions. Discard affected
outputs and restart the full review once from a new stable baseline. If the
baseline changes again, stop and ask the user to pause concurrent edits or
accept a later review; do not publish mixed-baseline results. A validation
command that changes a tracked file also triggers this stop. Never revert the
change automatically.

## Subagent orchestration

Use fresh child contexts. Every launch is asynchronous; use `wait()` when no
independent controller work remains. Do not end the skill with live children.
Keep review children read-only and do not use worktrees.

Use these gated stages rather than one monolithic chain, so a failed or drifting
stage cannot silently feed later stages:

1. **Standards research** — one `researcher`.
2. **Specialist fanout** — the ten roles in the review contract, in parallel
   with normal concurrency 5. Give every task the accepted standards artifact.
3. **Executable validation** — one `reviewer` after all ten accepted raw
   reports.
4. **Synthesis** — one fresh `context-builder`.
5. **Report validation** — one fresh read-only `reviewer`.

Every launch uses an attempt-specific output such as
`research/attempt-1.md`, `raw/R01-attempt-1.md`,
`validation/attempt-1.md`, `synthesis/report-attempt-1.md`, or
`synthesis/validation-attempt-1.md`. A fresh retry writes `attempt-2`; it never
resumes a child or overwrites an earlier artifact. Record the accepted artifact
path for each role and propagate those exact paths to every later stage and the
final report. Publication copies the accepted synthesis path, not a fixed
filename.

Set file-only outputs for large artifacts. Record every run id and output path.
Specialists do not run subagents. Use the exact role/output contracts from the
reference file in each task rather than a vague “review the repository” prompt.

Every research/review/validation task includes:

> Review only. Do not modify project/source files; returning evidence through
> the configured output artifact is allowed. Do not create plans, tickets, or
> implementation work, and do not run subagents. Inspect the current working
> tree. Never quote secret material, including secret-bearing fixture contents.

### Failure policy

Each mandatory researcher, specialist, validator, and synthesizer may be retried
once in a fresh child. Do not silently replace a missing angle.

- If research, a specialist, or executable validation still fails, stop and ask
  whether to continue with an explicitly limited report. The synthesis must
  name the accepted gap if the user agrees.
- If synthesis still fails, stop without publishing a report.
- If report validation fails, launch one fresh synthesis child with the rejected
  synthesis, validator findings, and all original inputs, writing a new attempt
  path; then launch one fresh report validator against that path. If it still
  fails, stop without publication.
- Timeouts, skipped checks, and unavailable tools are evidence gaps, not passes.

## Executable validation

The validation subagent receives standards research, all ten raw reports, and
their requested checks. It is the only child that runs expensive repository-wide
suites; specialists may run focused read-only checks.

It records status/manifest before and after; runs focused checks needed to test
candidate findings; and, for whole-repository scope, attempts every
non-redundant relevant command from `AGENTS.md`, including Rust format, Clippy,
workspace tests, SPA tests/build, browser flows inside `nix develop`, Nix checks,
and the VM test. It records exact commands, exit status, duration, non-secret
output summary, informed item ids, and every skip/timeout/unavailable reason.
It never repairs failures.

If a command changes a tracked file, stop further potentially mutating commands,
preserve the user's state, and report the delta without reverting it.

## Synthesis

The synthesis subagent receives:

- all research, raw review, and validation artifacts;
- current repository access and the baseline-manifest checksum;
- resolved scope and preflight commit/status metadata;
- active-plan inventory and historical ticket overlap;
- intended final report path;
- the prior report only in follow-up mode;
- the complete synthesis and report contract from the reference file.

It verifies every critical/high finding and disputed item against cited code,
docs, tests, or primary sources, plus a representative set of medium findings.
It deduplicates without erasing distinct perspectives, groups related findings,
calibrates priorities, builds the complete security-claim matrix, compares the
prior report when applicable, and gives every structured input item a final
ledger disposition.

The controller does not edit synthesis prose.

## Report-validation gate

The report-validator subagent reads the synthesis and every input artifact. It
returns pass/fail against all of these conditions:

- every required report section exists;
- all ten specialist roles and any accepted limitations are represented;
- every research observation, candidate finding, opportunity, verification gap,
  and validator-discovered item appears in the disposition ledger;
- every distinct normative security claim is present in the claim matrix;
- critical/high findings have verified evidence, confidence, and complete
  security-boundary fields when applicable;
- all partial, contradicted, and unverified claims link to a finding or explicit
  gap;
- low-impact findings remain present and deferred;
- opportunities remain separate from defects;
- active-plan overlap, historical ticket context, and follow-up comparison are present when applicable;
- no plan filename proposal, implementation plan, secret value, or unsupported
  claim appears;
- the baseline checksum and material limitations are stated.

A shape-only check is insufficient. Failed validation goes back to the synthesis
subagent; the controller never patches the report itself.

## Publication and presentation

After report validation passes and the baseline manifest is unchanged:

1. Copy the accepted attempt-specific synthesis artifact byte-for-byte to the
   preselected report path. Do not overwrite an existing report. Verify
   identical checksums.
2. Run `git status --short` and confirm the report is the only new intentional
   project change attributable to this skill. Do not alter unrelated changes.
3. Present in chat:
   - report path, raw artifact location, baseline checksum, and material limits;
   - executive synthesis and every remediation theme;
   - a complete index of every final finding id/title/priority/disposition,
     including deferred low-impact findings;
   - every opportunity id/title;
   - every unresolved or rejected item id/title;
   - security status counts and ids of every partial, contradicted, or
     unverified claim;
   - active-plan overlaps and historical-ticket comparison counts where
     applicable.
4. Explicitly state that no plans or tickets were derived or created, then stop.

Do not begin plan reasoning in the presentation. Plan work requires a later
explicit request through `writing-plans`.
