# Replace `mutable: false` with operator-toggleable mutation state per (wrapper, resource)

## Background

`docs/security.md` currently documents (lines 611–617):

> Secrets marked `mutable: false` should behave like write-once values.
> To avoid trivial time-of-check/time-of-use races during first write,
> underwrap serializes writes per secret so two concurrent writers
> cannot both observe "absent" and both win.

This is **not** implemented in the code. Integration test
`crates/integration-tests/src/tests/store_read.rs:98-123`
(`immutable_secret_rejects_overwrite`) asserts the opposite: "Second
store also succeeds — credentials are operator-managed and may be
overwritten." No `mutable` field is consulted anywhere in
`underwrapd` or the backend store write path; no write serialization
exists.

## Proposal

Drop the idea of a static per-secret `mutable` attribute entirely.
Replace it with an **operator-controlled, temporary "mutation allowed"
state** held in the policy document, keyed by `(wrapper, resource)`.
The admin UI becomes the primary surface for toggling this on and off
around a maintenance window; the default is "no mutation."

Rationale:

- The existing `PolicyRule` / `PolicyGrant` model in
  `crates/underwrap-protocol/src/policy.rs` is already keyed by
  `(wrapper, resource, operation)` and already supports
  time-bounded allow grants. "Mutation" is just another operation
  in that namespace.
- Immutability-as-schema was never implemented and adds a second
  axis of configuration that would drift from policy; folding it in
  removes the divergence.
- Operators already manage modes, rules, and grants centrally. One
  more "allow writes for the next 30 minutes" grant is consistent
  with how the rest of the system is administered.

Tradeoff to name in the docs: this makes immutability an
**operator-discipline property** rather than a schema invariant. A
permanent allow-grant behaves like `mutable: true` forever. If a
future requirement calls for true tamper-evidence ("this value was
never overwritten"), that has to come from a different mechanism —
append-only store, audit attestations — not from the policy layer.

## Scope of "writes" that need to be gated

Three code paths currently perform `store.write(...)` on secret
material and must consult the policy:

1. **Admin HTTP** — `/v1/admin/secret/write` in
   `crates/underwrap-backendd/src/server.rs:671-697`. Currently
   unconditional: `admin_store(...).and_then(|store| store.write(...))`.
2. **Wrapper-initiated libsecret writes** —
   `crates/underwrapd/src/secretsd_channel.rs:121` (backs the
   private Secret Service bus exposed to the target).
3. **Wrapper-initiated directory credential sync** —
   `crates/underwrapd/src/directory_channel.rs:129` (persists the
   target's managed directory tree back into the store).

The policy document itself is also stored under a reserved key
(`POLICY_DOCUMENT_SECRET_ID`) via `store.write`; that path must
**not** be gated (otherwise operators could lock themselves out by
disabling mutation). It is already separated because it goes through
dedicated admin policy endpoints, but keep this invariant in mind
while wiring the gate.

## Concrete changes

### Protocol (`crates/underwrap-protocol`)

- Introduce a canonical operation name for credential writes.
  Suggestion: `"store.write"` (parallel to the existing
  `"libsecret.read"` / `"libsecret.readwrite"` naming in
  `policy.rs`).
- Decide whether the resource key for admin writes is
  `env_resource(credential_ref)`, `directory_resource(...)`, or a
  new `credential_resource(...)` that subsumes all of them. I lean
  toward a single `credential_resource` so "can this credential
  currently be mutated?" has one answer per credential regardless of
  how it will be consumed.
- No new policy types needed: existing `PolicyRule` /
  `PolicyGrant` with `effect = Allow` is the mechanism.
- Document semantics: a write is permitted iff `evaluate(wrapper,
  resource, "store.write")` resolves to `Allow` — i.e. the default
  is deny, and the operator must explicitly grant.

### `underwrapd` (wrapper-initiated writes)

- `crates/underwrapd/src/secretsd_channel.rs`: before calling
  `store.write`, load the policy document and call
  `evaluate_after_approval_with(&policy, wrapper_name, &resource,
  "store.write")`. On `Deny`, emit `audit::store_denied` and return
  a scoped error to the secretsd client. On `NeedsApproval`, reject
  (secretsd channel cannot interactively produce an approval
  signature — the wrapper request that created the channel was the
  one authenticated moment).
- `crates/underwrapd/src/directory_channel.rs`: same gate, same
  audit.
- Reuse the `load_policy_document` / `evaluate_with` helpers in
  `crates/underwrapd/src/policy.rs`; avoid inlining policy
  evaluation.

### `underwrap-backendd` (admin writes)

- `POST /v1/admin/secret/write` currently takes
  `{store, secret_id, material_b64}`. It has no wrapper context.
  This is the main design decision: the admin path has to either
  (a) carry a `wrapper` parameter so the gate is applied the same
  way, or (b) have its own admin-scoped mutation-allowed flag.
- Recommended: (a). Extend `AdminSecretWriteRequest` in
  `crates/underwrap-protocol/src/service.rs` with an optional
  `wrapper: Option<String>` field. When set, the backend evaluates
  the policy gate against `(wrapper, credential_resource(secret_id),
  "store.write")` and rejects on `Deny`. When unset, reject outright
  — the admin UI should always specify the wrapper scope it is
  operating in.
- Keep bypass for `secret_id == POLICY_DOCUMENT_SECRET_ID` so
  operators cannot brick policy editing. (Even better: reject any
  admin write to the reserved ID via this endpoint; policy edits go
  through `/v1/admin/policy/put`.)
- Audit: emit `secret_stored` with the operator name on Allow and
  `store_denied` on Deny. The operator name is already available
  from `authenticate_admin` (`server.rs:532-562`).

### Admin API surface

- Either add a dedicated pair of endpoints
  (`/v1/admin/mutation/enable`, `/v1/admin/mutation/disable`) that
  wrap `grant/upsert` + `grant/delete` with a fixed
  `operation = "store.write"`, or rely on the existing
  `grant/upsert` / `grant/delete` and have the UI compose the
  correct tuple. Leaning toward the former for UX — operators
  should not have to know the operation string.
- The short-lived grant shape (`starts_at_epoch` /
  `expires_at_epoch`) already exists; the new endpoint just fills
  them in sensibly (e.g. "allow for 30 minutes from now").

### Admin UI (`underwrap-admin-ui`)

- Per credential in the wrapper view, add a "Mutation" control with
  states: "locked (default)", "allowed until HH:MM (expires in N
  min)", "allowed permanently (warn)".
- Toggling it calls the new admin endpoint. When enabling, default
  to a short expiry (30 min) with an override. Show a prominent
  countdown when active.
- Make the permanent-allow state visually distinct and require a
  second confirmation — it is the "I just built a `mutable: true`"
  escape hatch and should look like one.
- The admin UI's own "edit credential" form becomes a two-step
  flow: enable mutation → submit new value → (optionally) disable
  mutation again.

### Docs

- Rewrite the "Immutability enforcement" section in
  `docs/security.md` (currently lines 611–617) to describe the new
  model: default-deny writes, operator-toggleable allow grants,
  applicable to all three write paths (admin, secretsd, directory).
  Name the tradeoff explicitly (operator-discipline, not a schema
  invariant; no tamper-evidence).
- Update `docs/configuration.md` and the wrapper-author checklist
  if they still reference `mutable`.
- Delete the `mutable` field from the config-loader types if any
  still exist (audit: `grep -rn mutable crates/` returns only the
  integration-test harness at the moment).
- Rename the misleading integration test
  `immutable_secret_rejects_overwrite` → something like
  `overwrite_rejected_without_mutation_grant`, and make it assert
  the new policy gate (currently it asserts the opposite of what
  its name says).

## Open questions

1. **Operation namespacing.** `store.write` vs. per-path
   (`env.write`, `libsecret.write`, `directory.write`). Single
   operation is simpler and matches the "one mutation toggle per
   credential" UX; separate operations let operators grant
   libsecret-write without permitting admin overwrites. My default
   is single, but flag if the separate-operation shape is wanted.
2. **Admin-path wrapper parameter.** Requiring a wrapper on admin
   writes is clean but changes the API shape. Alternative: a
   `store-wide` grant keyed by `(store, secret_id)` with no wrapper,
   stored in the same policy doc. Probably uglier; raise it for
   completeness only.
3. **First-write semantics.** The old doc claimed writes were
   serialized so two racing writers couldn't both observe "absent"
   and win. That was never true in the code. Do we want it now? My
   read: no — the policy gate already funnels writes through the
   allow-grant path, and real concurrent admin writes are vanishingly
   rare. Document the non-guarantee rather than implementing it.
4. **`WrapperApprovalMode::PerCallApproval` + store writes.**
   secretsd/directory writes happen on the long-lived channel that
   was authenticated once at channel creation. Treat this as
   `approval_satisfied = true` (as reads do) and apply only the
   rule/grant layer. Decide and document.

## Acceptance criteria

- [ ] `mutable` field removed from all config/protocol types and
      the schema docs.
- [ ] Admin `POST /v1/admin/secret/write` rejects by default and
      only succeeds when a matching allow grant (or rule) is
      present, keyed by the wrapper and credential resource.
- [ ] `secretsd_channel` and `directory_channel` writes gated by
      the same policy operation, with `store_denied` audit on
      rejection.
- [ ] Policy edits (`/v1/admin/policy/put`) still work without
      needing a self-referential grant.
- [ ] Admin UI exposes an ergonomic enable/disable with a default
      short expiry; permanent-allow requires a confirm step.
- [ ] `docs/security.md` section rewritten; obsolete `mutable`
      references across `docs/` removed.
- [ ] `immutable_secret_rejects_overwrite` test renamed and flipped
      to assert the new behavior; add a parallel test for the
      secretsd and directory write paths.
- [ ] Manual check that expiry sweeper (`expiry_sweeper_loop` in
      `underwrap-backendd/src/server.rs:567`) drops expired
      allow-mutation grants the same way it drops expired
      `Unlocked { until }` entries. It already iterates
      `expires_at_epoch` on grants, so verify only, no new code.

## Out of scope

- Tamper-evidence / append-only storage.
- Operator-role scoping (already tracked in `TODO.md` under
  "operator-scoped tokens").
- Central audit forwarding of mutation toggles (will ride along
  with the existing audit-forwarding ticket in `TODO.md`).
