# Reject malformed libsecret attribute input

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-023
Source themes: C-005

## Outcome

The libsecret item form accepts only unambiguous `key=value` attribute text. Every nonblank line must contain a delimiter and a nonempty key, and each trimmed case-sensitive key may appear once. Malformed lines, empty keys, and duplicates produce line-numbered inline form errors and no request; valid attributes preserve the first-delimiter, trimming, case-sensitive lookup, blank-line, and empty-value semantics.

This corrects browser form validation only. The typed backend attribute map, Secret Service storage/search behavior, F-022 UTF-8 secret encoding, and arbitrary valid attribute keys/values remain unchanged.

## Scope

- Replace `parseAttributes`' silent ignore/last-wins behavior in `web/src/pages/secrets/libsecret/helpers.ts` with strict deterministic parsing:
  - split on lines; whitespace-only lines remain allowed and ignored;
  - every nonblank line must contain `=`; split at the **first** delimiter so values may contain additional `=` characters;
  - trim key and value as today; a trimmed empty key is invalid; an empty value is valid;
  - keys are case-sensitive, matching libsecret lookup semantics; `service` and `Service` are distinct;
  - reject a duplicate trimmed key and report both the duplicate line and original line/key;
  - return attributes through `Object.fromEntries` (or equivalent property-safe construction), not assignment into a plain `{}` that treats keys such as `__proto__` specially;
  - preserve source line numbers across blank lines and CRLF input.
- Add a small exported `AttributeParseError` (or equivalent typed result) with a stable reason (`malformed-line`, `empty-key`, `duplicate-key`), one-based line number, and non-sensitive operator message:
  - `Line N must use key=value.`
  - `Line N has an empty attribute key.`
  - `Line N repeats attribute key "K" from line M.`
  Do not include secret material; attribute keys are metadata, but messages should include only the relevant key and line numbers.
- Integrate strict parsing into `CreateItemDialog`'s Zod form validation:
  - add a field-level issue at `attributes` so `<FormMessage>` displays the exact parser message before submit;
  - valid form submission uses the strict parsed map in the existing typed API request;
  - malformed input makes no `postLibsecretItem` call and leaves the dialog open;
  - avoid a separate generic Alert for deterministic input errors;
  - keep F-022 `encodeUtf8Base64(v.secret)` and all id/label/content-type/duplicate-item validation unchanged.
- Add `web/src/pages/secrets/libsecret/helpers.test.ts` covering:
  - valid trimming, blank lines, CRLF, empty values, first-delimiter values, and case-sensitive near-miss keys;
  - malformed no-delimiter and empty-key errors with exact one-based line numbers;
  - duplicate after trimming with original/duplicate lines;
  - `__proto__`, `constructor`, and other object-like keys become ordinary own string properties without prototype mutation.
- Extend `create-item.test.tsx` with field-level rejection tests for malformed, empty-key, and duplicate attributes (no request), plus one valid multi-line request whose exact attribute map and F-022 Unicode `secret_b64` both remain correct.
- Synchronize `docs/admin-ui.md`, `docs/security.md`, and `docs/testing.md`: one `key=value` per nonblank line, first delimiter, trimmed key/value, empty values allowed, blank lines ignored, exact case-sensitive duplicate rejection, line-numbered UI errors, and object-key safety. Do not describe backend/API rejection that does not exist.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1220-attributes-red.py`, SHA-256 `330bfc63e9ba1600de93525088b4195d13c7bfbd5668064eed3836c9be1752f5`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1220-attributes-red.log`, SHA-256 `c634f164f1b4f286c6a04104fd1b5e46713023b65ecc56ccb858994d28f0c415`;
  - clean focused create-item baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1220-web-baseline.log`, SHA-256 `822865e08d714bccabe30096c946457c85a57e9fafd4a4fe481fbcec34c2f28c`.
  The script exits 1 with seven failing parser/test conditions; only the weak inventory that the form calls `parseAttributes` passes. The focused baseline passes the existing F-022 Unicode create-item test.

Expected implementation/product paths are limited to:

- `web/src/pages/secrets/libsecret/helpers.ts`
- new `web/src/pages/secrets/libsecret/helpers.test.ts`
- `web/src/pages/secrets/libsecret/dialogs/create-item.tsx`
- `web/src/pages/secrets/libsecret/dialogs/create-item.test.tsx`
- `docs/admin-ui.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Parser/form abstraction changes qualify as fix, not cleanup.

## Non-goals

- Do not change backend DTOs/handlers/OpenAPI, libsecret storage/search semantics, D-Bus validation, attribute key/value wire rules, resource limits, or content types.
- Do not reject blank lines, empty values, additional `=` in values, case-sensitive near misses, whitespace around keys/values, or object-like keys that can be represented safely.
- Do not add key syntax/length/reserved-name rules, Unicode normalization, case folding, sorting, escaping/quoting syntax, comments, multiline values, JSON input, or an attribute editor redesign.
- Do not change F-022 UTF-8/base64 utility or secret material handling. Do not add dependencies, log form text, or expose secret values.
- Do not absorb F-024 or any later finding, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Strict parser and inline rejection

- Nonblank malformed lines, empty trimmed keys, and duplicate trimmed case-sensitive keys are rejected with exact line-numbered messages. Blank lines/CRLF, empty values, first-delimiter values, trimming, and case-sensitive distinct keys remain valid.
- Parser output safely represents `__proto__`/constructor-like keys as own strings and cannot mutate its prototype.
- Create-item form shows the parser error at the attributes field and sends no request for every invalid class.

### Valid request and separation

- One valid mixed input produces the exact expected attribute map and still sends F-022 exact Unicode secret base64.
- Item-id/label/content-type validation, API request shape, backend behavior, and F-022 conversion remain unchanged.

### Documentation and scope

- Docs describe browser input validation, not a new backend guarantee.
- No backend/API/schema/D-Bus/storage/dependency/F-022 or adjacent F-id changes.
- F-023 is resolved. F-024+ and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: integrity and lookup semantics of libsecret item attribute metadata stored with a resource.
- Actor/fault: an operator supplies malformed or ambiguous line text; silent discard/overwrite makes a successful response claim metadata was stored when lookup-critical rows were omitted or replaced.
- Boundary direction: browser textarea -> strict line parser/inline validation -> typed attribute map -> backend/libsecret storage and later exact attribute lookup.
- Invariant: every accepted nonblank line contributes exactly one unambiguous case-sensitive key/value entry; invalid input produces no network mutation.

## Validation

- Re-hash and preserve the red/baseline artifacts above; run the desired-state script after the fix.
- `cd web && npm test -- src/pages/secrets/libsecret/helpers.test.ts src/pages/secrets/libsecret/dialogs/create-item.test.tsx`
- `cd web && npm test && npm run build`; remove generated schema/dist afterward.
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static inventory: no silent `continue`/last-wins/plain-object assignment; exact errors/tests; no `parseAttributes` changes outside approved paths; F-022 utility diff absent; no attribute/secret logging; exact staged allowlist; no generated/process artifacts.
- No E2E or VM test is required for this browser form-validation boundary. Stop if implementation changes backend/Secret Service/Nix/privileged behavior.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- Completed F-022 supplies exact Unicode `secret_b64` and remains unchanged.
- Existing backend/libsecret maps and searches attributes case-sensitively; this task validates only the textarea-to-map projection.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
