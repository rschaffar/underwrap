# RF-0005 — Manual store-cleanup command does not quote its path

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `L-01` · **Confidence:** High: source review, inert shell recording, handler regressions and desktop Chromium rendering/copy checks; no destructive command execution.

## Why it matters

A valid configured path containing spaces or shell syntax changes the suggested cleanup command.
An operator following it could remove unrelated paths or execute unintended shell syntax.

## Suggested fix

Present a correctly shell-escaped single operand with an option terminator, or provide only the literal path.
Keep registry-only deletion and deliberate manual cleanup.

## Decision and caveat

Fixed in `63a68753da4436e33c040dbc01302ac5e3a7f975` after the scoped acceptance checks passed.
Guidance retains manual cleanup and encodes one literal POSIX-shell pathname with an option terminator.
Registry-only deletion, retained generations and operator path verification remain intentional; no removal command is executed by the backend or shell witnesses.
Trusted configuration control is not a demonstrated attacker capability.

[RF-0005's checklist](finding.md#work-ownership) owns the joint implementation and verification with RF-0020.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
