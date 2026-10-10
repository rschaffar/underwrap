# RF-0024 — Fixture comment describes obsolete executable-keyed identity

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`nix/tests/node.nix:354-357` explains a thin libsecret fixture shim as necessary because two wrappers using one executable would share an identity.
A maintainer relying on that comment receives the old boundary model.

## Consequence and boundary

Current production generation uses `(instance, wrapper)`, not executable path (`nix/modules/underwrap.nix:619-635`).
`nix/tests/test.py:1800-1881` explicitly checks that same-binary wrappers have different identities.
The mismatch is in fixture explanation, not those runtime declarations/assertions.
Intentional real/debug variants of one pair sharing an identity remain separate from different wrapper pairs.

## Correction and alternatives

Align the comment with pair-keyed identity and explain any remaining shim purpose without the false security rationale.
A comment correction does not require deleting the shim, changing account derivation or weakening same-binary checks.

## Acceptance criteria

- Comment, module generation and maintained identity documentation agree on `(instance, wrapper)`.
- Keep same-binary/different-pair isolation assertions intact.
- If fixture behavior changes rather than only wording, run the normal VM gate; no runtime check is claimed necessary to prove the original wording mismatch itself.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
