# RF-0005 — Manual store-cleanup command does not quote its path

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Use a valid absolute data root containing whitespace or shell metacharacters, then force-delete a nonempty or locked store registry entry that returns cleanup guidance.
`resolve_store_dir` requires an absolute root but does not restrict it to shell-safe characters (`crates/underwrap-backend/src/admin_api/stores/mod.rs:193-221`).
The handler constructs an unquoted `rm -rf` suggestion by directly interpolating the selected data directory (`crates/underwrap-backend/src/admin_api/stores/crud.rs:600-605`).
The SPA presents that string for manual use (`web/src/pages/stores/dialogs/cleanup-hint.tsx:34-42`).

Shell parsing is not literal pathname transport.
A space-containing path becomes multiple operands; quotes, substitutions and command metacharacters can alter the program itself.
An option terminator alone does not quote the operand.

## Consequence and boundary

The protected asset is unrelated backend-host data.
The boundary is trusted configured pathname to suggested shell program to trusted operator execution.
An operator can delete the wrong files or run unintended syntax by following application-generated guidance.
No destructive execution occurred in the review, and no hostile caller control over the trusted root/record configuration was established.
This is Medium operator-safety correctness, not remote command execution by the backend or demonstrated privilege escalation.

Deletion intentionally removes registry ownership rather than the data tree or retained generations.
Selective operator cleanup remains required by `docs/operations.md:379-395,438-445`.
The separate claim that a YAML definition was removed is [RF-0020](../RF-0020/README.md), not a reason to automate cleanup.

## Correction and alternatives

If an executable shell command remains, encode the resolved path as exactly one literal shell operand and include an option terminator.
State the intended shell syntax.
Alternatively provide the literal path with selective cleanup instructions instead of a ready-to-execute command.
Preserve registry-only deletion and retained-generation warnings; do not delete collision occupants or lifecycle artifacts automatically.

## Acceptance criteria

- Assert exactly one intended literal pathname operand for spaces, single/double quotes, substitutions, metacharacters and admitted relative record paths after resolution.
- Use an inert argument-recording function or shell parser; never invoke real removal in the regression.
- Verify both returned guidance and displayed text; ordinary safe paths must still identify the same directory.
- Retain the option terminator if a command is supplied and confirm that neither expansion nor extra commands run.
- Preserve retained-data warnings and manual operator choice.
  No fixed-build check has been executed.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
