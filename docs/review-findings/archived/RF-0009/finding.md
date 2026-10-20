# RF-0009 — Text reveal and copy drop a leading UTF-8 BOM

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`decodeBase64Utf8` in `web/src/lib/utf8-base64.ts:22-43` decodes base64 to bytes and calls `new TextDecoder("utf-8", { fatal: true }).decode(bytes)` at lines 35-38.
With its default BOM handling, TextDecoder consumes a leading UTF-8 BOM rather than returning U+FEFF as text.
The browser's own encoder at lines 3-16 can produce such bytes from valid input, so this is not confined to externally malformed data.

`web/src/pages/secrets/reveal-secret-view.tsx:64-72,122-129` uses the decoded string both for the reveal textarea and for `navigator.clipboard.writeText`.
Flat env/files and libsecret item reveal share that view.
The relevant prefix is the UTF-8 signature EF BB BF; no resource value is needed to describe the trigger.

## Consequence and boundary

The helper returns `ok: true` while losing one leading code point.
No warning distinguishes the normalized value from an exact decoding.
Copying it, using it as a credential, or submitting it as replacement material can therefore lose meaningful content.
This read/copy path itself does not rewrite the stored bytes.

Exact text handling is promised by `docs/backend.md:64-66`, `docs/operations.md:295-303`, and `docs/testing.md:366-370`.
Ordinary multilingual and empty inputs, and rejection of malformed base64 or non-UTF-8 bytes, are useful existing controls but do not cover a leading BOM.
Intentional text-only behavior and lack of binary download are not findings.

## Correction and alternatives

Configure or implement strict decoding so a valid initial U+FEFF survives successful text conversion.
Keep malformed representations distinct from missing or empty values.
Silently normalizing the prefix is inconsistent with the present exact-text contract; changing that contract would be a separate product decision, not an equivalent correctness fix.
No change to arbitrary-byte backend storage is required.

## Acceptance criteria

- Demonstrate exact encoder/decoder equality for a leading U+FEFF, including a BOM-only value and ordinary-text controls.
- Exercise the shared reveal/copy flow for flat and libsecret values, comparing in memory without printing the material.
- Retain empty-value behavior, chunk-safe encoding, and strict invalid-base64/invalid-UTF-8 failures.
- Confirm that a read/copy neither mutates storage nor silently transforms successful text.
  Completed correction checks are recorded in [Evidence](evidence.md#resolution-evidence), separately from the discovery observations.

## Work ownership

This file owns the user-authorized RF-0009-only correction based on local `main` at `c7c3c3d771fedffb1755942e55a219e6c1afb827`.
Scope is the shared frontend decoder and exact reveal/copy regressions; backend storage, binary editing, dependencies, unrelated findings and form lifecycle redesign are excluded.
Implementation and verification use a dedicated feature branch and worktree.

- [x] Add a failing leading-U+FEFF regression with BOM-only, ordinary Unicode, empty and invalid controls.
- [x] Correct the authoritative decoder while preserving strict rejection and chunk-safe encoding.
- [x] Verify flat env/files and libsecret reveal and real clipboard equality, with unchanged stored material and no value-bearing diagnostics or captures.
- [x] Run full Vitest, selected API/library coverage, production build, browser-fixture checks and relevant same-tree real-backend Playwright flows inside the pinned Nix environment.
- [x] Record evidence and limits and archive after acceptance.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
