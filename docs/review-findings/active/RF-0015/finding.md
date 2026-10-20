# RF-0015 — Parallel fixtures use non-reentrant group lookup storage

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`crates/integration-tests/src/harness/builders.rs:10-17` implements `current_group_name()` by obtaining effective GID, calling `libc::getgrgid`, then dereferencing the returned record/name with `CStr::from_ptr` and copying it to an owned String.
The ownership is established only after the copy, not while the shared libc result is being read.

Two fixture/test threads can call the helper concurrently; callers include `crates/integration-tests/src/harness/mod.rs:302` and `crates/integration-tests/src/tests/mod.rs:330`.
Default Cargo parallelism permits this even when an individual test uses a current-thread Tokio runtime.
The cooperative backend startup reservation has already been released before the helper call and does not serialize these reads.
Another NSS lookup can overwrite shared getgrgid storage between return and completion of the Rust copy.

## Consequence and boundary

Possible consequences are data races, invalid pointer/string observations or intermittent wrong-group fixture setup within the test process.
A final owned String does not repair the preceding unsafe lifetime.
The helper also lacks the pointer/lifetime safety explanation required by `CONTRIBUTING.md`.

The relevant fixture contract is to select the process's effective-GID group so in-process daemon connections satisfy its gate.
This is not evidence that production daemon NSS has the same defect: `docs/local-protocol.md:18-21` explicitly describes reentrant caller-owned output in production.
The supported peer-authentication boundary is not shown bypassed.

## Correction and alternatives

Use an NSS interface whose result buffer remains privately owned for the whole record/name copy, with explicit null/result/error and buffer-lifetime handling.
Preserve effective-GID selection and deliberately handle lookup failure rather than silently changing identity semantics.
A local lock would need to cover every competing use of shared NSS storage; serializing only one test runtime or relying on the startup reservation is not a sufficient demonstrated remedy.

## Acceptance criteria

- Establish a documented safety argument covering the returned record, name pointer and copy lifetime.
- Exercise repeated concurrent group lookups/fixture setup under Cargo parallelism, checking stable correct effective-group results without sensitive output.
- Cover missing/error/buffer-growth behavior as appropriate to the selected API, and retain ordinary fixture setup controls.
- Use race/sanitizer evidence where supported, without treating a clean stress run alone as proof of safe FFI ownership.
  Track current verification through the shared work owner and this finding's evidence record.

## Work ownership

The user-authorized bounded fixture task uses [RF-0014's shared scope/evidence checklist](../RF-0014/finding.md#work-ownership).
This finding retains its own NSS acceptance and archival decision.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
