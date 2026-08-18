# Encode and reveal secret text as strict UTF-8

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-022
Source themes: C-005

## Outcome

The browser's existing text secret UI maps JavaScript strings to exact UTF-8 bytes before base64 transport and maps base64 bytes back to text only when they are valid UTF-8. Unicode flat and libsecret writes no longer throw or corrupt, Unicode reveals/copies round-trip exactly, malformed base64 and valid non-UTF-8 secret bytes produce distinct visible warnings, and an empty secret is distinguishable from a missing value.

This preserves the existing text-only UI and arbitrary-byte backend contract. It does not invent a binary editor or reinterpret non-UTF-8 bytes; it fails visibly rather than showing/copying mojibake.

## Scope

- Add one dependency-free browser utility, `web/src/lib/utf8-base64.ts`, as the sole production conversion boundary:
  - `encodeUtf8Base64(text)` uses `TextEncoder` to obtain UTF-8 bytes, converts bytes to a binary string in bounded chunks (no spread/call-stack overflow at the resource-size limit), then uses `btoa` only on that byte string;
  - `decodeBase64Utf8(value)` first uses `atob`, copies each binary code unit into `Uint8Array`, then uses `new TextDecoder("utf-8", { fatal: true })`;
  - decoding returns a typed discriminated result: `{ ok: true, text }`, `{ ok: false, reason: "invalid-base64" }`, or `{ ok: false, reason: "invalid-utf8" }`;
  - empty base64 decodes successfully to empty text; null/undefined remains the caller's missing-value case;
  - no Node `Buffer`, dependency, implicit Latin-1 conversion, logging, or secret retention cache is added.
- Add focused utility tests with independent known vectors:
  - `"✓ à la mode"` → `4pyTIMOgIGxhIG1vZGU=`;
  - `"päss🔐"` → `cMOkc3Pwn5SQ`;
  - Japanese and empty-string cases;
  - a large multi-byte input that exceeds one chunk and round-trips without argument-stack failure;
  - malformed base64 (for example `%%%`) yields `invalid-base64`;
  - valid base64 for bytes `0xff 0xfe` (`//4=`) yields `invalid-utf8`, never replacement characters.
- Replace direct `btoa(material)` in both flat create/update paths, including rename-with-replacement, with `encodeUtf8Base64`.
- Replace direct `btoa(secret)` in libsecret item creation with `encodeUtf8Base64`.
- Replace `RevealSecretView`'s direct `atob`/null collapse with strict typed decoding:
  - `material_b64 == null` remains “does not exist/was removed”;
  - valid empty base64 renders an empty read-only textarea and can be copied as empty text;
  - `invalid-base64` shows a server-contract/malformed-transport warning;
  - `invalid-utf8` states that the value contains non-UTF-8 bytes and cannot be displayed or copied by the text UI;
  - only a successful decode can reach the textarea or clipboard; copy preserves exact Unicode text.
- Extend `web/src/pages/secrets/dialogs/write.test.tsx` with known-vector Unicode create and rename/update request assertions while preserving existing ASCII cases.
- Add focused libsecret item creation tests that submit Unicode secret text and assert exact UTF-8 base64 in the typed API body. Do not couple these tests to the separately queued F-023 attribute-validation change.
- Add focused `RevealSecretView` tests for Unicode reveal/copy, empty value, malformed base64, and valid non-UTF-8 bytes, with query cache cleanup and clipboard isolation.
- Synchronize `docs/admin-ui.md`, `docs/security.md`, and `docs/testing.md`: browser secret inputs are UTF-8 text encoded losslessly; reveal is strict/fatal; arbitrary backend bytes remain supported but non-UTF-8 cannot be rendered/copied by this text UI; the two error categories and test vectors are pinned.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1210-utf8-base64-red.py`, SHA-256 `25091fc580e21333379d23ddace378695f34acca96183ac6915d3e6e3fe21fc7`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1210-utf8-base64-red.log`, SHA-256 `e705ec9b9ef865b2cee30a5729f74e491c8b79e178ef3ffad59e2e36f0b81cd9`;
  - clean focused write-dialog baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1210-web-baseline.log`, SHA-256 `f226f298cfbb6464908fd635efdebe7a742f8ecef2aeb4b772bddf898987dc38`.
  The script exits 1 with all seven conditions absent; the log additionally records Node's direct Unicode `btoa` `InvalidCharacterError`. The focused baseline passes two existing ASCII write tests.

Expected implementation/product paths are limited to:

- new `web/src/lib/utf8-base64.ts`
- new `web/src/lib/utf8-base64.test.ts`
- `web/src/pages/secrets/dialogs/write.tsx`
- `web/src/pages/secrets/dialogs/write.test.tsx`
- `web/src/pages/secrets/libsecret/dialogs/create-item.tsx`
- new sibling create-item test
- `web/src/pages/secrets/reveal-secret-view.tsx`
- new sibling reveal view test
- `docs/admin-ui.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Shared test rendering helpers qualify only if independently useful before and after this fix.

## Non-goals

- Do not add a binary/hex/base64 editor, download mode, replacement-character decoding, content-type inference, line-ending normalization, Unicode normalization, password masking redesign, or arbitrary-byte text coercion.
- Do not change backend resource bytes, DTOs, API/OpenAPI, size limits, storage, libsecret protocol, content types, or base64 wire format.
- Do not use Node `Buffer` in production browser code, add a dependency/polyfill, log secret text/bytes/base64, or persist conversion results outside existing form/query lifetimes.
- Do not change libsecret attribute parsing/validation; F-023 remains separate. Do not absorb F-024 or any later finding, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Exact UTF-8 transport

- No production secret write/reveal path calls `btoa` or `atob` directly; the sole utility uses them only after/before explicit byte conversion.
- Flat create, edit, rename-with-material, and libsecret item creation send known exact UTF-8 base64 for Unicode input. ASCII remains byte-identical.
- Utility known-vector, empty, multi-byte, large-chunk, malformed-base64, and fatal-invalid-UTF-8 tests pass.

### Strict text reveal

- Valid Unicode renders and copies exactly. Empty bytes render as empty text rather than missing/error.
- Missing value, malformed base64, and non-UTF-8 bytes are three distinct states. Invalid cases never reach textarea/clipboard and never show replacement mojibake.
- Query caching/GC and clipboard behavior remain otherwise unchanged.

### Documentation and scope

- Current docs distinguish UTF-8 browser text from arbitrary backend bytes and state both decode failures without promising a binary UI.
- No backend/API/schema/size/storage/libsecret-attribute/dependency/generated-output or adjacent F-id changes.
- F-022 is resolved. F-023+, binary editing, Unicode normalization, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: exact secret bytes and integrity across operator text -> browser UTF-8 -> base64 -> backend bytes, and backend bytes -> strict browser text/copy.
- Actor/fault: an operator supplies valid Unicode, or the backend returns arbitrary/malformed bytes; implicit Latin-1 `btoa`/`atob` must not throw, corrupt, or silently replace bytes.
- Boundary direction: JavaScript string <-> explicit UTF-8 bytes <-> base64 wire value <-> backend arbitrary-byte storage.
- Invariant: successful text operations are byte-exact UTF-8; bytes that are not valid UTF-8 are reported, not altered or exposed as misleading text.

## Validation

- Re-hash and preserve the red/baseline artifacts above; run the desired-state script after the fix.
- Focused utility, flat write, libsecret create-item, and reveal-view tests.
- `cd web && npm test && npm run build`; remove generated schema/dist afterward.
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static inventory: direct production btoa/atob only inside utility; exact known vectors/error branches/call sites; no secret logging; no F-023 attribute parser diff; exact staged allowlist; no generated/browser/process artifacts.
- No E2E or VM test is required for this dependency-free browser conversion boundary; stop if implementation changes backend/API/Nix/privileged behavior.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- Completed F-009 defines the complete 2-MiB resource contract; chunked encoding must not introduce a smaller browser call-stack limit.
- Existing backend DTOs carry standard base64 and preserve arbitrary bytes. Existing reveal GC/confirmation/copy behavior remains the UI authority.
- F-023 owns libsecret attribute validation and remains untouched.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
