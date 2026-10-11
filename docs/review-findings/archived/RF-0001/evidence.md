# RF-0001 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The discovery observations below come from the comprehensive review on 2026-10-08, not checks performed during the initial documentation import.
Review aliases: `N-01`.
Original review owners: `namespace.md`, `local-boundary-verification.md`, `local-boundary-commands.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
The initial record import executed no correction; the resolution section records subsequent correction verification.

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

## Discovery limits

At the review baseline, no packaged distinct-UID replacement, raw-secret recovery, arbitrary privileged write or authenticated bus interception was executed.
Universal preclaimability and disk persistence are not established.
The original confidentiality-oriented wording is superseded by the final High integrity/containment disposition.
The original review's fresh mount-kernel source retrieval failed with HTTP 429; that review claimed no such newly retrieved source.
The initial record migration ran no behavioral checks and implemented no fix.

## Resolution evidence

Resolution: `fixed` at `365916646f67bda32fcd1ab356baaeead1361925`, following the main correction in `5a989d6b275265f1997202cef4aacdf685e77de5`.
The discovery evidence above remains scoped to its original baseline.
Correction verification uses the project Nix development environment and flake-pinned `x86_64-linux` NixOS VM, with caller U distinct from the provisioned nonroot target X.

### Implemented authority and retained regressions

`crates/underwrap-ns/src/mounts.rs::create_directory_staging_tmpfs` returns the actual owned `fsmount` descriptor for one mandatory `noswap`, 8 MiB tmpfs.
`directory_handle.rs::Directory::from_mount` retains that descriptor through attachment to a checked runtime-directory object in the private namespace, then opens its root for traversal.
Ordinary directory descriptors alone do not preserve anonymous mount namespace membership when the actual `fsmount` file closes.
`directory_credential.rs::prepare_directory_mounts` retains projection handles before fork; materialization, `open_tree`/`move_mount` binding and collection use those objects rather than staging paths.
The relay drops its handles before helper/target forks, while the host retains collection authority through the unchanged lifecycle and backend finalization checks.
Setup reestablishes checked non-dumpability inside the target-fsid window and after restoration, including setup errors.

`crates/underwrap-ns/src/directory_credential/tests/replacement.rs` verifies ordinary-directory ancestor replacement before materialization and after staging, replacement of a projection name, repeated collection, symlink/special-file refusal and mismatched-plan rejection.
`directory_handle/tests.rs` verifies CLOEXEC, component rejection, exact owner/group/mode checks, mode-zero attachment handles and restrictive-umask handling.
These ordinary-directory tests establish handle retention and traversal behavior, not private tmpfs or cross-UID behavior.

`nix/tests/test.py::files_staging_provenance`, with `nix/tests/node.nix`'s target fixture, exercises the shipped daemon, wrappers, namespace binary and backend.
U preclaims and renames the legacy basename before launch, then replaces it again only after the target's READY barrier confirms original contents and completed binding.
The target verifies X ownership, directory/file modes, a shared tmpfs device for attachment and projection, reported `noswap` and 8 MiB cap, zero target capabilities and no unexpected inherited descriptors.
Root instrumentation separately sees an empty mode-zero bare runtime attachment; U cannot rename it.
The release barrier keeps the target running through replacement, after which it reconfirms its original view and optionally authors a change.
Four controls passed: writable-changed, writable-unchanged, read-only-changed and a root-protected legacy basename.
Each compares the complete backend blob and exact terminal audit verb, so target exit alone cannot hide suppressed finalization or substitute a UsageSignal for a required Checkin.

### Executed checks

| Command | Tested inputs and observed result |
| --- | --- |
| `nix develop --command cargo nextest run --locked -p underwrap-ns` | Final correction inputs: exit 0, 136 passed, 0 skipped; 2.455 seconds reported by nextest. |
| `nix develop --command cargo clippy --locked -p underwrap-ns --all-targets -- -D warnings` | Final correction inputs: exit 0. |
| `nix develop --command cargo fmt --all -- --check` | Final correction inputs: exit 0. |
| `./crates/integration-tests/run tests::wrapper_writeback` | Main correction revision: exit 0, 12 passed, 102 filtered; 3.53 seconds for tests. The later setup-guard-only delta leaves these backend/daemon inputs unchanged. |
| `nix build --no-link --print-out-paths .#checks.x86_64-linux.vm-test` | Final correction revision: exit 0; all four provenance controls passed within the complete VM script, which reported 148.83 seconds. |

Durations other than the reported test timings were not measured.
To repeat the packaged verification, run the VM command at the fixed revision; its existing script provisions synthetic resources, identities and barriers without external probe files.
The namespace command includes the retained replacement and descriptor tests.
No secret-bearing output or temporary execution artifact is required by this record.

### Development failures and remaining limits

Two earlier VM prototypes rejected before target execution with `EINVAL`: cloning without a namespace attachment, then attaching after prematurely closing the actual `fsmount` owner.
The accepted implementation attaches first while that descriptor is live; the full VM now exercises subsequent cloning after it closes.
A development parallel `cargo test -p underwrap-ns` run timed out in the existing `tests::initial_publication_reject_and_channel_failure_are_pre_exec_no_terminal` test.
Its exact isolated rerun, the complete serial suite and fresh-process nextest suites passed; the parallel timeout's cause was not established.
Final namespace acceptance uses fresh-process nextest rather than claiming default parallel-runner reliability.

The minimum Linux 6.5 kernel was source-cross-checked for mount-descriptor lifetime and permission API compatibility, but not independently executed.
The VM verifies the corrected implementation, not a packaged exploit against the original reviewed revision.
This is not evidence of direct secret recovery, arbitrary privileged writes, authenticated bus interception, comprehensive syscall-failure injection, secure erasure or an independent security audit.
Caller-controlled destination names can still affect target path reachability; they do not select the host's retained staging objects.
Libsecret bus pathname hardening and backend policy changes remain outside the authorized correction.
