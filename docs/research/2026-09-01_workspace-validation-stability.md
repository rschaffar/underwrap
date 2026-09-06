# Workspace validation stability

Date: 2026-09-01.

Status: Concluded for the release candidate.

## Scope

This investigation covers failures discovered while running the clean release matrix after the VM and SPA release-gate corrections.

It covers test and coverage evidence only.

No production protocol, dependency, privileged binary chain, authorization rule, or security boundary changed.

## LLVM profile merge failure

The first complete candidate check ran every instrumented Rust test successfully and then LLVM 22.1.6 `llvm-profdata merge` terminated with `SIGILL`.

Passing `--num-threads=1` reproduced the same termination and disproved the initial threading hypothesis.

The retained coverage run contained 49 raw profiles.

Merging each profile separately found exactly one failing input.

That file was owned by the build user but had mode `0066`, so the owner's permission class had no read bit and an ordinary read returned `Permission denied`.

The failing profile belonged to the backend test executable that re-execs one exact rotation test after setting `umask 0700`.

An exit-time profile created with the ordinary `0666` creation mode under that umask receives mode `0066`.

LLVM's crash rather than a normal unreadable-input diagnostic is tool behavior, but the project-side cause was the unreadable test-child artifact.

The discarded serial-merge experiment made no tracked change.

## Profile correction

The restrictive-umask child now keeps `umask 0700` throughout the rotation operation and assertions.

A child-local drop guard changes the exit umask to owner-only `0077` only when that exact single-test process returns or unwinds.

This preserves the property under test while allowing exit-time instrumentation to create an owner-readable profile.

A focused instrumented run produced a child profile with mode `0600`, and direct merging of every focused raw profile succeeded.

The Nix `workspace-llvm-cov` check then generated its report successfully.

## Descriptor-observation failures

The first corrected coverage run reached the test phase and exposed an intermittent target-status failure.

The target-status tests dropped their writer and immediately treated EOF as proof that no writer remained.

A parallel test can spawn a child during that interval, and the child can temporarily inherit the process-global writer before `exec` applies `FD_CLOEXEC`.

The EOF-dependent target-status witnesses now execute in fresh single-test processes.

The malformed `SO_PEERPIDFD` payload test later exposed the corresponding numeric-descriptor race in an ordinary workspace run.

It closed an acquired descriptor and then asserted `F_GETFD == -1`, but another parallel test could reuse that descriptor number before the assertion.

That close witness now executes in a fresh single-test process while retaining the exact `EBADF` assertion.

A bounded inventory found one remaining equivalent numeric close witness in the `underwrap-exec` relocation test.

That witness was isolated proactively before the final candidate matrix.

The earlier libsecret endpoint-selection correction already used the same process-isolation principle for its numeric descriptor checks.

These corrections strengthen the evidence boundary rather than reducing concurrency, retrying failures, delaying checks, or weakening an assertion.

## Cross-process backend startup correction

The initial backend-port correction held a process-local asynchronous mutex across the probe listener's release-to-bind window and HTTPS readiness.

That serialized ordinary integration fixtures in one libtest process but did not coordinate separate nextest processes.

The bounded independent review identified this as a Minor validation-reliability finding.

Correction `efecdb1373fdd04e5f5600f5ebc31ba49670fc13` retains the asynchronous mutex and adds a close-on-exec advisory file lock shared by cooperating processes with the same temporary directory and effective UID.

The reservation retains both guards until HTTPS readiness, and a fresh child-process witness verifies cross-process exclusion, release on drop, and descriptor non-inheritance.

An unrelated process that does not take the advisory lock can still claim the released loopback port, so the mechanism does not claim universal kernel-level reservation after the probe listener closes.

## Validation

Focused regular and instrumented restrictive-umask tests passed.

Direct merging of all focused raw profiles passed.

The target-status tests passed, followed by five complete default-parallel runs of all 125 `underwrap-ns` tests.

The malformed pidfd close test passed, followed by five complete default-parallel runs of all 68 `underwrapd` library tests.

The relocation close test passed, followed by five complete default-parallel runs of all 14 `underwrap-exec` tests.

Rustfmt and denied-warning Clippy passed for every affected package.

The cross-process correction's focused child-process test, all 107 integration tests, integration-test Clippy, and the Nix `workspace-nextest` check passed.

The final pre-integration clean candidate matrix at `f61d8ddeb76de76dad0e4c8bffc45dcee7c0bd9e` passed the direct workspace tests and the complete Nix flake check, including `workspace-llvm-cov`.

After the required rebase onto `main`, exact candidate `5aae8810d1e941e214e4494057d8c750bcc0f0e4` differed only in `AGENTS.md` workflow guidance and passed the complete direct matrix again.

The final coverage output is `/nix/store/bshm2i2vsdp32c1smbqv79ln41ksc25d-underwrap-llvm-cov-0.1.0`.

## Residual observations

A diagnostic `nix build --rebuild` ran all instrumented tests and generated the report, then reported that the regenerated LCOV output differed byte-for-byte from the prior output.

Byte reproducibility of execution-count coverage reports was not an acceptance row and was not established.

The coverage report is a check artifact rather than a shipped product artifact.

LLVM 22.1.6 still handles an unreadable raw profile incorrectly; the correction prevents this repository's restrictive-umask child from producing that input.
