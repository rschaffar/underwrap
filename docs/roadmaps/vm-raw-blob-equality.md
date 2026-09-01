# VM raw-blob equality investigation

Status: Future direction.

## Current evidence

The NixOS VM test contains byte-for-byte backend resource comparisons around F-007 near-cap libsecret lifecycle flows and F-009 exact-maximum complete-resource flows.

Exact-tree runs during F-032 intermittently stopped at these existing comparisons after the relevant wrapper path had otherwise reached its expected terminal state, and a clean detached checkout reproduced one F-007 failure without the F-032 daemon change.

A later F-052 run stopped once at an F-009 comparison and passed completely on an immediate same-tree rerun.

A later F-054 run stopped once at an F-007 comparison and passed completely on an immediate unchanged-tree rerun, while the subsequent F-055 VM run passed on its first attempt.

These observations establish an intermittent test obligation, not a deterministic current product defect, a known harness root cause, or evidence that canonical backend bytes may safely differ.

On 2026-09-01 the user closed this obligation from the F-031 through F-058 remediation program into this roadmap without claiming that it is fixed or resolved.

## Direction

A future grounded investigation should first obtain a bounded reproduction and identify whether the mismatch comes from test observation order, deployed lifecycle sequencing, canonical resource assembly, backend persistence, or another evidenced cause.

The protected asset is the integrity of backend-managed canonical resource bytes, and the unknown failure source lies across the deployed target, namespace, daemon finalization, backend persistence, and VM observation path.

Retain the current equality assertions unless evidence supports a separately reviewed stronger test; do not weaken, delete, retry around, or hide them merely to produce a green run.

Instrumentation may record identifiers, lengths, digests, section names, and event ordering, but it must never print resource blobs, fixture values, canaries, key material, or other secret-bearing content.

Promote this direction into a grounded active plan before implementation, with explicit authority for any product, protocol, storage, wrapper-chain, or security-boundary change and with repeated NixOS VM evidence appropriate to the identified cause.

This roadmap grants no implementation authority.
