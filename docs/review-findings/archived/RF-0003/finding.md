# RF-0003 — Structured admin libsecret identifiers fail initial publication

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

In an otherwise valid resource/collection, create an item with its optional ID omitted.
The ordinary SPA does this when the field is blank and advertises the `item_1` convention (`web/src/pages/secrets/libsecret/dialogs/create-item.tsx:71-87,112-118`).
The backend allocates `item_<n>` and saves successfully (`crates/underwrap-backend/src/admin_api/libsecret.rs:362-374`).
Its shared identifier admission also allows arbitrary-length item tokens and dots/dashes in collection/alias segments (`crates/underwrap-backend/src/admin_api/libsecret.rs:62-95,223,524`).

Strict publication instead requires exactly 32 lowercase hexadecimal characters for item IDs and ASCII alphanumeric/underscore collection and alias segments (`crates/underwrap-protocol/src/resource_blob.rs:238-273`).
Admin save and ordinary resource loading canonicalize/bound the resource without imposing those projection rules.
Backend open, daemon decoding/cache/Checkout and secretsd conversion preserve identifiers; they do not translate them.
Generation zero must pass strict host validation before secretsd readiness and target execution (`crates/underwrap-secretsd/src/main.rs:155-160,244-257`; `crates/underwrap-ns/src/publication.rs:136-168`).

## Consequence and boundary

A normal successful admin operation prevents the entire libsecret-enabled invocation from starting until the incompatible entry is removed/recreated, also blocking access to otherwise valid existing items in that resource.
Env/files-only wrappers need not fail.
No hostile input, oversized resource, race or malicious target is required.

This merges A-01's backend/storage/transport trace, S-02's conversion and initial-readiness consequences, and W-01's default UI omission and permissive validation evidence.
Medium reflects workflow scope without demonstrated confidentiality or authorization failure, rather than uncertainty about the mismatch.
Dot-segment browser URL normalization remains a separate consequence in [RF-0008](../../active/RF-0008/README.md).

`docs/backend.md:44-60` deliberately separates representability from projectability.
That limit does not justify a built-in structured producer whose default item ID always violates its intended consumer's contract.
This record does not require every raw ResourceBlob load or disabled section to pass projection validation.

## Correction and alternatives

Align generation and new-entity admission across backend, UI and schema descriptions with the existing strict identifier contract.
Retain inspection/deletion/repair of previously stored invalid entities.
Do not weaken host validation or silently rename identifiers during Checkout.

### Contextual non-finding: random-ID collision wording

SecretStore random IDs use 128-bit OS randomness without collision retry (`crates/underwrap-secretsd-store/src/store/items.rs:1-28,58-110`).
The collision-proof wording is too strong: the scheme is collision-resistant, not mathematically collision-proof.
No practical collision exploit or attacker-controlled overwrite was established.
This is a retained non-finding qualification, not a new accepted vulnerability or an approved generation change.

## Acceptance criteria

- Connect real default admin creation through stored-state loading, strict generation-zero acceptance, private-service readiness and lookup; CRUD-only success is insufficient.
- Cover omitted and explicit item IDs, length/case/nonhex boundaries, collection/alias punctuation and valid underscore/alphanumeric controls.
- Rejected new identifiers must leave prior resource state unchanged; existing invalid state must remain repairable.
- Preserve unrelated sections/entries, env/files-only behavior, strict publication references/canonicality and existing policy gates.
- Do not infer full startup success from the recorded isolated validator control.
  Fixed-build acceptance now includes the deployed startup/readiness/lookup witness recorded in [resolution evidence](evidence.md#resolution-evidence).

## Work ownership

This finding owns the user-authorized correction of admin-created libsecret identifiers.
Keep the strict host contract, raw storage/load semantics, disabled projections and existing read/delete repair paths unchanged.
Do not redesign publication, rename stored identifiers implicitly, add dependencies, or resolve unrelated findings.

- [x] Share identifier predicates between strict publication and admin admission; generate compatible default item IDs without overwriting existing entries.
- [x] Align SPA validation and generated API descriptions; cover omitted/explicit IDs and rejection before mutation.
- [x] Verify legacy inspection/deletion, unrelated state preservation and env/files-only behavior.
- [x] Exercise default admin creation through persisted loading, generation-zero validation, private-service readiness and lookup.
- [x] Run the affected Rust, generated-contract, SPA/browser, integration and VM checks and record their actual evidence.

Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
