# RF-0005 — Manual store-cleanup command does not quote its path

**Priority:** Medium · **State:** in-progress

**Review aliases:** `L-01` · **Confidence:** High: source-confirmed interpolation and independent harmless shell-tokenization witness; no destructive command or handler execution.

## Why it matters

A valid configured path containing spaces or shell syntax changes the suggested cleanup command.
An operator following it could remove unrelated paths or execute unintended shell syntax.

## Suggested fix

Present a correctly shell-escaped single operand with an option terminator, or provide only the literal path.
Keep registry-only deletion and deliberate manual cleanup.

## Decision and caveat

The authorized correction retains manual cleanup and encodes one literal POSIX-shell pathname with an option terminator.
No automatic deletion or configuration-policy redesign is included.
The backend does not execute the command.
Trusted configuration control is not a demonstrated attacker capability.
The witness replaced rm with a harmless function and removed nothing.

[RF-0005's checklist](finding.md#work-ownership) owns the joint implementation and verification with RF-0020.
Final disposition follows completed acceptance checks.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
