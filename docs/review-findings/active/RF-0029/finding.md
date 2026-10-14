# RF-0029 — Failing equality assertions can print unwrapped fixture material

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

A byte-equality or decoded-value assertion fails at `crates/integration-tests/src/tests/store_read.rs:299-307,346-354,369-377,433`.
These selected `assert_eq!` operands include ordinary canonical `Vec<u8>`, decoded byte buffers or strings.
Rust's equality-assertion failure path formats both operands; once a value is unwrapped, its original secret wrapper's redacted Debug implementation cannot protect it.

Additional examples were identified at `crates/integration-tests/src/tests/daemon.rs:885` and `crates/integration-tests/src/tests/rotation.rs:74-80,114-118`.
This is a selected-source finding, not a complete inventory of every logging sink or assertion.

## Consequence and boundary

An ordinary regression can move local fixture/resource contents into durable test diagnostics readable by CI artifact consumers.
No hostile production actor is needed for that failure path.
Only synthetic fixture use was inspected; there was no executed failed assertion and no observed production-secret leak.

`docs/testing.md:269` forbids printing resource bytes, decoded buffers, keys or failing secret-bearing assertions for diagnosis.
`AGENTS.md` applies confidentiality discipline to fixtures as well.
A green suite avoids the failure branch during that run but does not establish that its diagnostics are safe.
This is distinct from production error-envelope or log-redaction assurances.

## Effect of RF-0001 and additional affected comparisons

At `7d4b1d30794171956031cef45b805b0c46500619`, `crates/underwrap-ns/src/directory_credential/tests.rs:37-47` still compares unwrapped byte slices using operand-formatting `assert_eq!` in `assert_round_trip`.
The [RF-0001](../../archived/RF-0001/README.md) replacement regressions call this existing helper at `crates/underwrap-ns/src/directory_credential/tests/replacement.rs:15,19,54`, extending the same failure-diagnostic pattern to the new coverage.
This supplements the original selected integration-test sites; it is not a separate finding or evidence of an executed leak.

The new VM complete-blob comparison at `nix/tests/test.py:3121-3123` uses exact equality with a value-free failure message.
That safe contrast does not repair the shared Rust helper or the original integration assertions.
The inspected helper uses synthetic fixture data, and no deliberately failing resource assertion or live disclosure was executed.

## Correction and alternatives

Keep exact complete-resource equality as the correctness condition while replacing operand-formatting diagnostics with safe decision/length/representation metadata.
Merely weakening the comparison, comparing only lengths, or relying on a wrapper after bytes have already been decoded would not preserve the intended evidence.
Value-free complete-resource comparisons already exist in `crates/integration-tests/src/tests/client_access_override.rs:212-221` and migration tests.

## Acceptance criteria

- Check both matching and mismatching comparisons without exposing either operand.
- In an authorized isolated diagnostic check, use deliberately nonsecret sentinels rather than forcing an existing secret-bearing fixture assertion to fail.
- Confirm the safe failure message does not contain raw bytes, decoded values or value-bearing Debug output while exact inequality still fails.
- Audit the listed integration sites and the namespace `assert_round_trip` helper, including its replacement-test callers; preserve exact equality, repeat-collection checks and independent test outcomes.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
