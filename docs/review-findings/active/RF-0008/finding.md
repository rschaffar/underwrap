# RF-0008 — Accepted dot-only libsecret IDs normalize out of request paths

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

The optional collection/item ID grammar in `web/src/pages/secrets/libsecret/helpers.ts:7-35` accepts exactly `.` and `..`: both satisfy `[A-Za-z0-9._-]+`, the length limit, and the prohibition on a leading underscore.
Backend admission agrees at `crates/underwrap-backend/src/admin_api/libsecret.rs:66-95,214-244,348-388`.
Creation carries the chosen ID in JSON; subsequent collection/item operations place it in a URL path.
Canonical storage at `crates/underwrap-backend-store/src/resource_blob.rs:48-58` does not impose the strict publication identifier grammar.

The API templates are in `web/src/api/libsecret.ts:14-18,37-52`; item reveal also reaches a path-based endpoint through `web/src/pages/secrets/libsecret/dialogs/reveal-item-secret.tsx:40-41`.
The installed client serializes a primitive path parameter with `encodeURIComponent` and constructs a `Request`.
That encoding does not protect dot-only segments from URL normalization.

## Consequence and boundary

A collection DELETE with ID `.` loses that segment; ID `..` also removes the preceding `collections` segment.
An item-secret GET with item ID `..` removes the preceding `items` segment.
Successful JSON creation therefore does not establish that later supported browser operations address the created object.

This is a browser/admin addressing defect, distinct from the generation-zero publication failure in [RF-0003](../RF-0003/README.md).
It does not require a wrapper invocation.
No broader unauthorized operation or cross-store authorization bypass was demonstrated; a changed route shape is not proof that another privileged operation succeeds.

## Correction and alternatives

Require every admitted ID to survive the complete browser/client/router path literally.
Rejecting dot-only segments in the authoritative producer and matching UI validator is one bounded approach.
A different addressing scheme would need end-to-end proof; merely adding URI encoding is not sufficient without checking normalization.
Preserve safe supported identifiers and strict publication validation.
Handling already-stored problematic IDs needs an explicit choice rather than silently making them unreachable.

## Acceptance criteria

- Check collection and item `.`/`..` admission on both UI and backend, including direct admin requests.
- With ordinary-ID positive controls, verify create, read/reveal and delete address exactly the intended record through the actual browser and backend.
- If any problematic ID remains accepted, demonstrate its literal transport round trip and absence of route confusion.
- Keep publication compatibility, backend authority and existing safe-ID behavior intact.
  These checks have not been performed against a correction.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
