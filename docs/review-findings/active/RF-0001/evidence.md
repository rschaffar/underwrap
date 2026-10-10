# RF-0001 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `N-01`.
Original review owners: `namespace.md`, `local-boundary-verification.md`, `local-boundary-commands.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The original namespace review and later independent local-boundary verification trace mount setup, U-to-X transition, materialization, fsuid collection and backend finalize.
The later verifier completely read 24 selected production files, including the module and actual entry chain, and inspected the pinned nixpkgs wrapper implementation.
Relevant documented promises are `docs/security.md:95-101`, `docs/local-protocol.md:178-182,267-271` and `docs/configuration.md:163-174`.

## Recorded observations

A prior nonsecret external Python witness used an unprivileged user/mount namespace, privately mounted a temporary directory, checked that its contents were absent from the parent view, then renamed/replaced the bare directory from the parent.
A later staging-shaped child pathname write reached the replacement.
Reported results were `child_setup=READY`, `child_exit=0`, `private_mount_hidden_in_parent=true` and `synthetic_write_reached_host_backing_directory=true`; the command exited 0 within its 20-second tool timeout.
Elapsed duration was not measured.

This was the load-bearing mount/relookup primitive, not Underwrap execution.
The witness maps the same host user into a user namespace; it does not reproduce distinct-UID DAC or persistence.
Independent verification inspected that prior witness but did not rerun it or mount anything.
It source-verified that regular readable replacement files can be collected without DAC-bypass capabilities.

Existing `crates/underwrap-ns/src/directory_credential/tests.rs:1-224` covers routing/static symlinks, and `nix/tests/test.py:1690-1767` covers ordinary staging.
The coordinator's fresh VM check passed in 174.22 seconds but did not preclaim/replace the staging root.
Neither that pass nor exact lifecycle-gate tests establish filesystem-object provenance.

## Reproduction or verification procedure

The original probe was external, not a repository test; no surviving probe file is assumed.
To reconstruct its bounded method in an authorized disposable environment:

1. Create an empty synthetic bare directory and hold the parent-side namespace view.
2. In a private mount namespace, mount synthetic tmpfs and verify a marker is hidden from the parent.
3. Rename the bare dentry from the parent, recreate its old name, and ask the child to perform a later pathname operation.
   Observe only identity/result booleans.
4. Separately test the actual shipped chain with distinct U/X and deterministic post-bind replacement as specified above.
   The prior primitive does not substitute for this step.

## Limits and missing evidence

No packaged distinct-UID replacement, raw-secret recovery, arbitrary privileged write or authenticated bus interception was executed.
Universal preclaimability and disk persistence are not established.
The original confidentiality-oriented wording is superseded by the final High integrity/containment disposition.
A fresh mount-kernel source retrieval failed with HTTP 429; no such newly retrieved source is claimed.
This migration ran no behavioral checks and implemented no fix.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
