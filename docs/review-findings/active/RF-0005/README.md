# RF-0005 — Manual store-cleanup command does not quote its path

**Priority:** Medium · **State:** open

**Review aliases:** `L-01` · **Confidence:** High: source-confirmed interpolation and independent harmless shell-tokenization witness; no destructive command or handler execution.

## Why it matters

A valid configured path containing spaces or shell syntax changes the suggested cleanup command.
An operator following it could remove unrelated paths or execute unintended shell syntax.

## Suggested fix

Present a correctly shell-escaped single operand with an option terminator, or provide only the literal path.
Keep registry-only deletion and deliberate manual cleanup.

## Decision and caveat

Open, Medium operator safety.
No automatic deletion or configuration-policy redesign is authorized.
The backend does not execute the command.
Trusted configuration control is not a demonstrated attacker capability.
The witness replaced rm with a harmless function and removed nothing.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
