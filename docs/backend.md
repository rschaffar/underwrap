# Backend reference

`underwrap-backend` owns resources, policy, and durable brokered-resource storage.
Runtime, admin API, and the embedded SPA share one HTTPS listener.
Use [Configuration](configuration.md) for settings and [Operations](operations.md) for deployment, enrollment, key custody, maintenance, backup, recovery, and decommissioning.

## Data model

| Entity | Authority and relationship |
| --- | --- |
| Store | Stable id, operator metadata, canonical directory, backend kind, and independent live lifecycle state |
| Runtime client instance | Active or disabled enrolled leaf fingerprint, exactly one current store, an `auto_create_wrapper_assets` flag defaulting off, and a client-wide access override defaulting to Individual settings |
| Resource | Stable server-generated `res_*` id, operator-facing name, and write-once creation provenance |
| Wrapper/resource pair | Wrapper name plus stable resource id, write-once provenance, and optional `last_used_at` |
| Permission row | Runtime client instance, wrapper, stable resource id, permission mode, writable flag, and optional reason |
| Operator | Backend-wide administrator with an Argon2id verifier; no per-store roles |

Resources can be renamed without changing their ids or existing permission tuples.
Provenance does not change on later registration or rename.
A frontend wrapper binds one resource and enables a nonempty combination of env, files, and libsecret projections.
There is no per-secret runtime read filter inside that authorized resource.
Local users are not policy principals, and `allowedUsers` is not authorization.

Store and runtime-client records live in backend-managed registries, not inline store definitions in backend YAML.
Store records use one JSON file per stable id under `stores_dir`; the file stem supplies the id.
Operator records and bootstrap state are separate from brokered-resource storage.
Missing registry directories are treated as empty collections, allowing initial deployment before enrollment.

The selected `StorageBackend` contains:

```text
resources/<resource_id>  complete canonical resource
control/resources       resource catalog and provenance
control/pairs           wrapper/resource pairs and usage timestamp
control/policy          permission rows
control/canary          encrypted backend only
```

`control/` is the reserved internal-key prefix used by content filtering and store emptiness probes.
Runtime DTOs cannot name any of these storage keys.

## Resource format

A resource is one RFC 8785 JCS-canonical JSON value with `schema_version: 1` and inline `env`, `files`, and `libsecret` sections.
Env and files map names to base64-encoded arbitrary bytes.
Libsecret contains aliases, collections, items, labels, content types, attributes, timestamps, and arbitrary secret bytes.
The complete canonical resource has an inclusive **2,097,152-byte** limit, excluding outer HTTP/base64 and local framing.

Collection and item `created` and `modified` fields each admit unsigned integers from zero through `9,007,199,254,740,991` (`2^53 - 1`).
These four bounds ensure canonical numeric representation, not temporal plausibility or created-before-modified ordering.
The shared [`ResourceBlob` implementation](../crates/underwrap-protocol/src/resource_blob.rs) owns the format.

A resource below the complete-size limit is not necessarily projectable.
Env projection additionally requires allowed names and UTF-8 values without NUL.
Files need the namespace's complete routing and collision plan.
Libsecret readiness needs a strict publication graph and a separately bounded canonical section, inclusive at **1,048,576 bytes**.
[Local protocol](local-protocol.md#parsing-and-copies) distinguishes typed parsing, exact canonical-byte checks, copies, and projection validation.

Flat admin entry routes support env and files, not libsecret; structured libsecret routes preserve its graph.
Each mutation loads and changes the complete aggregate, preserving unrelated sections rather than reconstructing it from a lossy flat view.
A missing catalog resource produces the canonical value-free `404 not_found` / `unknown resource` response before blob access.
An existing resource with no active blob begins as empty; a missing flat key has a nullable read and idempotent deletion, not the same missing-parent response.

The browser editor is text-only, although backend storage remains byte-preserving.
It encodes exact UTF-8 without normalization and distinguishes missing, empty, malformed-base64, and non-UTF-8 values.
Malformed or non-UTF-8 values are not placed in reveal or clipboard output.
Browser attribute-textarea validation is an input convenience, not a redefinition of the backend or D-Bus typed map contract; see [Operations](operations.md).

## Permission model

One permission row is keyed by `(runtime client instance, wrapper, resource id)`.
The saved writable flag is independent of permission mode.
The following matrix governs individual rows; the client-wide override below determines when those saved settings apply.

| Permission mode | Real variant | Debug variant |
| --- | --- | --- |
| `Locked` | deny (`locked` or supplied reason) | deny (`locked` or supplied reason) |
| `Unlocked` | allow | deny (`not-unsafe-debug`) |
| Active `UnlockedUntil` | allow | deny (`not-unsafe-debug`) |
| Expired `UnlockedUntil` | deny (`expired`) | deny (`expired`) |
| Active `UnsafeDebugUntil` | allow | allow |
| Expired `UnsafeDebugUntil` | deny (`expired`) | deny (`expired`) |

Expiry is evaluated on every open and finalize, without a background sweeper.
The locked reason can be returned verbatim, so it must not contain secrets.
The daemon selects the configured real target or debug tool only after the backend authorizes the requested variant.
A forged real variant cannot select the debug tool.

Both operations authenticate the current runtime client instance and acquire an available-store lease, then resolve the resource and exact pair before evaluating its override and saved permission.
An absent resource or pair denies `unregistered-pair` even under Unlock all.
A writeback additionally requires the current effective writable flag or denies `not-writable`.
There is no global per-store write toggle: authenticated admin edits are not governed by runtime writable flags.

### Client-wide override

The override is persisted on the backend runtime client instance, not in the daemon configuration or policy document.
It applies to registered pairs in that instance's current store, including pairs registered later.

| Override | Ordinary access | Debug access | Effective writable flag after admission |
| --- | --- | --- | --- |
| Individual settings | Saved row and mode; absent row denies `no-permission` | Saved active `UnsafeDebugUntil` row only | Saved row flag |
| Lock all | Deny `client-locked` | Deny `client-locked` | False |
| Unlock all | Allow registered pairs, including absent, locked, or expired ordinary rows | Saved active `UnsafeDebugUntil` row still required | Override flag |

Unlock all neither creates nor extends unsafe-debug approval; its writable flag also applies to separately approved debug calls.
Saved permission rows, reasons, deadlines, and writable flags remain unchanged by override updates and resume their effect in Individual settings.
Disabled identities, unavailable stores, registration requirements, canonical/size validation, and immutable env remain independently enforced.

Admin list/update responses expose `access_override` as `{"kind":"individual"}`, `{"kind":"locked"}`, or `{"kind":"unlocked","writable":false}` with either boolean allowed for Unlocked.
Only Unlocked accepts a writable field, and that field is required and non-null.
New instances and persisted records with no override default to Individual settings.
Omission or null in an admin update leaves the override unchanged.

Each open and finalize resolves the current override with the authenticated principal; no daemon restart, new runtime endpoint, or additional runtime response field is needed.
The existing Allow/Checkout writable flag records open-time effective writability.
A later writable override does not upgrade a supported frontend invocation opened read-only: it still discards staged changes instead of sending Checkin.
A writable invocation can attempt Checkin after a later denial, but finalize reauthorizes against current backend settings.
This request-time snapshot does not recall delivered material, terminate targets, or transact with concurrent administration.

Read-only files and libsecret surfaces are still writable staging for the target.
Changes are discarded rather than persisted; disclosure and target behavior are unchanged.
Unsafe debug deliberately permits a shell or command with the full projection, including for any local user who can invoke the authorized instance/pair.
Use [Security](security.md#policy-outcomes-and-diagnostics) and [Wrappers](wrappers.md) when assessing that capability.

## Runtime API

The runtime API has exactly three POST routes.
All derive the principal and store from mTLS, and carry wrapper/resource names rather than store ids, slot paths, or storage prefixes.
Requests remain parser-untrusted after authentication.
The wire types live in [`service/runtime.rs`](../crates/underwrap-protocol/src/service/runtime.rs), separately from generated admin OpenAPI.

| Route | Request and outcome |
| --- | --- |
| `/v1/wrappers` | Register configured wrapper/resource pairs at daemon startup; resolve existing assets or create missing ones when the principal's auto-create flag allows it |
| `/v1/wrapper-invocations` | Open names plus `real` (default) or `debug`; return Allow with resource id, base64 resource, writable flag, or a structured Deny |
| `/v1/wrapper-invocations/finalize` | Reauthorize names and variant; omitted blob reports usage, supplied blob attempts full-resource replacement |

Registration validates names and rejects an exact repeated Rust-string `(wrapper_name, resource_name)` row as `400 duplicate_wrapper_resource_pair` before consulting auto-create or storage.
Names are not trimmed, normalized, or case-folded for duplicate detection; sharing only one tuple component is valid.
Repeating an accepted pair in a later request is idempotent.
Registration is upsert-only, never removes or modifies existing entities or provenance, and seeds a missing permission as Locked.
With auto-create disabled, unresolved assets are reported as `auto-create-disabled` with missing resource/pair lists rather than silently created.
Enrollment or configuration replacement needs the explicit daemon restart described in [Operations](operations.md); lazy client construction does not repeat startup registration.

The daemon caches the **original decoded resource bytes**, not the outer base64 string, and returns those bytes on Checkout.
The backend normally reparses stored resources and canonicalizes open responses; this does not make daemon parsing an independent proof of exact JCS or projectability.

### Finalize and concurrency

Finalize is stateless: it does not depend on a backend open-session record.
A usage signal is allowed only if the current policy would allow that invocation.
A writeback rechecks writability, base64 decoding, decoded size, complete typed resource and timestamp parsing, schema version, exact canonical bytes, and env equality with the resource snapshot loaded during finalize.
It does not add files routing or strict libsecret publication-graph validation.

Invalid-blob outcomes distinguish `decode-failed`, `resource-too-large`, `schema-unknown`, `noncanonical-blob`, and `env-section-immutable` from policy Deny outcomes.
A successful storage save atomically replaces the complete value, then attempts to update usage metadata.
`last_used_at` is also updated after a successful open and usage-only finalize, at one-second resolution; failure to update it does not fail an otherwise successful operation.
It is not evidence that changed content persisted.

Atomic replacement gives visibility of one complete value, not a transaction around load/compare/save.
Concurrent resource and control-document mutations are **last-writer-wins**.
In particular, env equality against finalize's loaded snapshot does not protect an admin env edit that races between that load and the whole-resource save.
There is no resource revision, CAS, global mutation lock, deduplication, or automatic retry.
Serialize participating runtime writeback, admin content, resource, pair, permission, registration, and deletion writers; use [Operations](operations.md) to establish maintenance quiescence.

With an authorized local lifecycle, finalization also runs after nonzero target exits or signal death.
Finalization errors are diagnostic and preserve valid target status.
A refused Checkin can lead to a subsequent UsageSignal; neither local protocol nor HTTP provides single-use or exactly-once persistence.
[Runtime-client scheduling](architecture.md#runtime-client-scheduling) describes the daemon's admission limit, deadline, and ambiguous remote outcomes.

## Registry and entity consistency

Registry persistence success precedes publication of changed live authority, but the locking mechanisms differ:

- Store collection mutations use a separate collection-serialization lock, short live-registry lock scopes, and exact entry/generation revalidation around persistence.
- Runtime-client mutations hold the client live-registry write lock across their persistence helper, restoring the in-memory candidate on helper failure before releasing it.
- Migration, rotation, update, and delete acquire the entry-owned lifecycle mutex; lock and unlock revalidate the exact entry and lifecycle state under their final registry lock.

Do not infer that every registry write lock spans filesystem I/O.
Within the participating registry mutation, helper failure leaves live authority unchanged; this is not cross-registry coordination or a filesystem durability guarantee.
Registry writes sync a temporary file and atomically replace it without parent-directory fsync, and deletion is an ordinary unlink.
Store creation can leave a backing directory or encrypted canary if later registry persistence fails; key validation happens before those create-side effects.

Runtime-client remap and deletion commit their new authority before best-effort old-permission cleanup.
Cleanup failure can accompany success and stale rows can remain for operator inspection.
It does not restore old-store access through a remapped principal, but stale grants can matter to later reassignment or identifier reuse.
There is no rollback of the committed mapping just because cleanup failed.

For serialized admin requests, explicit content operations require the selected-store resource before blob access, pair creation requires that resource, and permission upsert checks client/store assignment, resource, and exact pair in that order before policy access.
Disabled runtime clients remain valid references.
Creating a pair with a new resource saves two documents separately and only attempts rollback if the second save fails.
The checks and later uses can race other writers; there is no transaction, tombstone, post-delete exclusion, automatic scan, repair, or garbage collection.
Unfiltered content lists and permission list/delete remain diagnostic and cleanup surfaces for pre-existing inconsistencies.

### Deletion

Resource deletion loads the complete typed blob and all pair and permission blockers.
Any env entry, file entry, libsecret alias, collection—including an empty one—or item blocks deletion, as do referencing pairs or permission rows.
The response combines counts and capped deterministic identifier-only samples, never values, labels, alias targets, attributes, timestamps, or secret bytes.
Identifiers can still be sensitive and are not copied into logs.

Once all blockers are absent, deletion removes the active empty resource key before saving catalog removal.
A missing key is the valid never-materialized empty case.
Success follows both operations; a later catalog-save failure does not reconstruct the blob.
A storage error does not establish which effects became visible: reread active key and catalog before serialized reconciliation.
A racing writer can recreate an orphan, and retained ciphertext generations or storage media are not erased.

Store deletion removes registry ownership, not its canonical directory or retained rotation workspaces.
Its optional `force` parameter defaults to false; ordinary nonempty/mapped conflicts are 409, and locked contents cannot be certified empty.
Use the server action projection and [Operations](operations.md), not a false `has_secrets` value on an unavailable store, to decide whether deletion is appropriate.
Outside-contract forced env entries can block both ordinary per-key repair and resource deletion; recovery requires trusted-host known-good restore/replacement or store rebuild, not a deletion shortcut.

## Store lifecycle

`PlainBackend` uses plaintext files; `EncryptedBackend` uses ChaCha20-Poly1305 and a 32-byte master key held in process memory.
Backend kind is separate from lifecycle state and server-derived available actions.

| State | Meaning and available actions |
| --- | --- |
| `ready` | Plain storage available; edit, migrate, and usage-dependent delete |
| `unlocked` | Encrypted storage and key available; edit, migrate, rotate, lock, and usage-dependent delete |
| `locked` | No installed encrypted backend; edit, unlock, and conservative force-delete |
| `busy` | Migration owns lifecycle publication; no mutating action |
| `recovery-required` | Migration state requires trusted-host recovery; no mutating action |

Unavailable stores expose conservative unknown-content usage, not inspected emptiness.
Storage access returns `503 store_locked`, `503 store_busy`, or `500 migration_recovery_required` as applicable.
The latter takes precedence over `same_backend` when migration recovery is pending.

Encrypted stores start locked on restart.
Unlock verifies an encrypted canary; lock removes the registry's installed backend reference without draining admitted leases.
The master key is zeroized when its final owner drops, not necessarily when lock returns.
Unlock and post-promotion reopen require an existing canonical directory rather than creating an empty substitute after an interrupted swap.

### Keys

The authenticated, CSRF-protected store-key endpoint generates a fresh key without mutating storage and returns no-cache response headers.
Encrypted creation and plain-to-encrypted migration require exact, untrimmed 64-character lowercase hexadecimal input before mutation and do not return the key again on success.
Plain creation and encrypted-to-plain migration accept no key; explicit JSON `key_hex: null` is invalid rather than equivalent to omission.
Unlock alone tolerates surrounding whitespace.
Rotation instead generates its replacement key during the mutation and returns it once afterward.
Underwrap writes no master-key file; external generation-specific custody is the operator's responsibility.
For confirmation, ambiguous response handling, and restart/unlock verification, follow [Operations](operations.md).

### Migration

Migration changes backend kind at the unchanged canonical path while preserving stable store id, metadata, client mappings, resource/storage ids, control documents, and logical bytes, except backend-specific canary presence.
It publishes `busy` on the exact entry, blocks new complete-operation storage leases, and waits up to 30 seconds for admitted leases to drain.
A drain timeout restores unchanged source availability and returns `503 store_busy` before filesystem mutation.
No registry lock spans waiting, storage I/O, filesystem mutation, or record persistence.
After drainage, the synchronous copy/verify/promote/persist/cleanup/publication window uses `block_in_place` on the production multi-thread runtime and has no asynchronous cancellation point.
Earlier invocations can still finalize later under the stateless contract, so operators must quiesce wrapper traffic and admin mutations.

A collision-safe sibling `.<store-id>.underwrap-migration-<16-lowercase-hex-id>` contains a non-secret versioned manifest, `stage/`, and later `backup/`.
Workspace and new stage roots must be non-symlink directories of exact mode `0700`; backup inherits source mode inside the private workspace, and mode drift can require recovery.
Occupancy, directory type, and symlinks are checked before rename/removal.

Migration keeps a sorted id inventory, copies and drops one value at a time, and compares one source/target value pair at a time with complete id-set and canary checks.
Its bounded source-authority evidence is a domain-separated SHA-256 fingerprint covering source kind, canary treatment, sorted ids, unambiguous lengths, and every logical value byte—not a whole-store plaintext snapshot.
This is not a collision-impossibility or malicious-backend-host-root guarantee.
After displacement, it reopens the same-kind source at `backup/` without exporting encrypted keys and revalidates that fingerprint before promotion, around moving a promoted target aside, and after source restoration.
Direct one-pair byte comparison remains required before and after promotion.

After target reopen and verification, migration persists only the record's backend-kind change, removes the displaced source and workspace, publishes the new live entry, and returns success.
An ordinary phase error is returned only if the verified source and source record remain authoritative and secret-bearing workspace cleanup succeeds.
Otherwise it preserves remaining artifacts and requires recovery.

| Failure class | Public result |
| --- | --- |
| Reservation, initial listing, or canary validation | `migrate_stage_failed`, `migrate_list_failed`, or `migrate_canary_failed`, subject to cleanup disposition |
| Copy read/write or target verification failure with source authority revalidated | `migrate_read_failed`, `migrate_write_failed`, or `migrate_verify_failed`, subject to verified source restoration and cleanup |
| Source-fingerprint capture/revalidation fails, including unreadable values or id mismatch | `migration_recovery_required`, even before source displacement |
| Swap, target reopen, or record persistence failure | `migrate_swap_failed`, `migrate_reopen_failed`, or `write_store_registry_failed` only after verified restoration and cleanup; otherwise `migration_recovery_required` |
| Failed rollback, ambiguous state, or secret-bearing cleanup failure | `migration_recovery_required` |
| Exact migration workspace found at startup | `migration_recovery_required`; no automatic selection or deletion |

The phase errors in the table are fixed value-free HTTP 500 responses.
An unreadable source is not universally a copy-read error: source-authority failure takes the recovery path.
Plain-to-encrypted failure can retain plaintext in `backup/`; encrypted-to-plain failure can retain plaintext in `stage/` or the canonical path.
Successful plain-to-encrypted migration removes intentional plaintext migration pathnames, not necessarily recoverable media blocks.

### Rotation and recovery limits

Rotation shares the lifecycle mutex but **does not drain ordinary I/O**.
Concurrent reads can fail and writes can be lost; quiesce access before starting it.
It stages and verifies a fresh encrypted generation, promotes with individually atomic renames, and retains displaced ciphertext in unique mode-`0700` `.<store-id>.underwrap-rotation-<16-lowercase-hex-id>` siblings.
Successful rotation leaves the old generation in `backup/`; a failed but rolled-back attempt can retain unpublished `stage/`.
Failed rollback preserves useful artifacts and can lock the store with `rotation_recovery_required`.

Rotation workspaces are retained indefinitely and are not selected, reconciled, overwritten, or cleaned by startup, subsequent rotation, response delivery, resource deletion, or store deletion.
Each remains readable with its generation key and consumes disk until deliberate trusted-host cleanup.
Unlike rotation retention, any remaining exact migration workspace blocks ordinary storage access pending recovery.

Recovery is a stopped-backend, trusted-host task requiring preservation, checked non-overwriting quarantine/promotions, matching generation keys, and verification of the chosen state.
Migration candidate selection requires qualified tooling built against the matching backend implementation to enumerate and read every logical value; canary unlock alone is insufficient.
No self-service generation inspector, automatic selector, or cleanup command is shipped.
[Operations](operations.md) owns the procedure and unsupported specialist prerequisites.
Neither lifecycle operation guarantees response delivery, external key custody, CAS, parent-directory fsync, power-loss durability, secure erasure, missing-key recovery, or recovery of media loss, arbitrary corruption, or already partially removed artifacts.

## Admin contract and errors

The generated [OpenAPI 3.1 document](../crates/underwrap-backend/openapi.json) describes annotated admin routes, DTOs, security requirements, and responses.
It is not a runtime-API schema, a complete middleware/extractor error inventory, or an enforcement mechanism for allowed origins.
Public login, whoami, and read-only bootstrap status are distinguished from protected reads and session-plus-CSRF mutations.
There is no network operator-bootstrap route or per-store detail route; store lists carry lifecycle/action projections, not a per-kind directory detail object.
Last-user deletion reports `409 last_user_protected`.

Generated structure, byte equality, and TypeScript compilation establish only their checked properties.
Semantic assertions cover selected contract meaning; live handlers/router tests remain authoritative for actual status and error behavior.
A stale annotation can remain type-correct and need not break the SPA build.
Rust OpenAPI drift tests can rewrite tracked JSON before failing, whereas npm generates ignored TypeScript output; regeneration instructions belong in [Testing](testing.md).

Generic admin and runtime internal failures return `{code, message}` with code `internal` and fixed message `the backend could not complete the request` at HTTP 500.
Audited registry/storage errors retain specific codes with fixed public guidance; arbitrary source detail is not returned to clients.
See [Security](security.md#policy-outcomes-and-diagnostics) for type-only logging and [Operations](operations.md) for diagnosis rather than interpreting target success or timestamps as writeback receipts.
