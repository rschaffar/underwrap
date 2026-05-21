# Task: Documentation sweep for the new runtime API

Parent: `600_story_runtime_api_redesign.md`

## Scope

Update every prose surface that describes the runtime API, the
policy model, the wrapper-approval flow, or the daemon's trust
posture. The codebase already enforces the new model after the
earlier tickets; the docs must catch up.

### `docs/architecture.md`

- Replace the "Two-stage wrapper chain" runtime-API description.
  The daemon talks to the backend via exactly:
  - `POST /v1/wrappers` (register)
  - `POST /v1/wrapper-invocations` (open)
  - `POST /v1/wrapper-invocations/finalize` (finalize)
- Remove the SSH-approval section. Replace with a one-sentence
  pointer: "Per-invocation approval is out of scope; use OS-level
  mechanisms (sudo / polkit) if you need a prompt."
- Add a "Trust boundary" subsection that enumerates what the daemon
  CAN and CANNOT do (mirroring the epic's trust-boundary summary).
- Update the diagram so the runtime API surface shows the three
  endpoints, not the seven old ones.

### `docs/backend.md`

- Rewrite the "Runtime API" section against the new endpoint set.
  Document request / response shapes, error codes, the
  env-immutability rule, the canonicality rule.
- Remove every reference to `secret_id`, `/v1/read`, `/v1/write`,
  `/v1/list`, `/v1/read-prefixes`, `/v1/delete`, `/v1/status`, and
  the old `/v1/runtime/wrappers/register`.
- Update the policy-document section: `WrapperApprovalMode` is
  `{ Locked, Unlocked, UnlockedUntil }`. Drop the
  `PerCallApproval` description and the `approval_key` field.
- Update `WrapperResourcePairRecord`: no `files_mount_paths`.
- Document the `last_used_at` bump semantics on both open and
  finalize.

### `docs/security.md`

- Update "Design invariants":
  - The runtime client never names a storage key. The wire shape
    cannot address `__underwrap/*` or `<resource>/<slot>/<key>`.
  - Policy evaluation is server-side. The daemon carries no policy
    state.
  - SSH-agent per-call approval is not a feature of this product.
  - The env section of a resource blob is mutable only via the
    admin API; runtime writebacks may not change it.
- Update the "Never do" list with:
  - Never reintroduce a `secret_id` field on the runtime wire.
  - Never reintroduce client-side policy evaluation.
- Update the "Always do" list with:
  - Re-evaluate the policy on every open and every finalize
    server-side.
  - Validate every runtime-writeback blob (decode + re-canonicalize
    + env-equality) before persisting.
- Remove the SSH-approval section. The threat model paragraph
  about "approval bypass" goes away because the feature does.

### `CLAUDE.md`

- Rewrite the "Architecture notes" subsections affected:
  - "Per-call approval" — delete the whole subsection.
  - "Wrapper permission model" — update the mode list, drop
    `PerCallApproval`.
  - "Resource storage and writeback model" — update to say "writes
    flow daemon → `POST /v1/wrapper-invocations/finalize` →
    backend" instead of the per-call IPC bridge language.
- Update the "Key files to understand first" list:
  - Drop `crates/frontend-underwrapd/src/approval.rs`.
  - Drop `crates/frontend-underwrapd/src/policy.rs`.
  - Drop `crates/frontend-underwrapd/src/wrapper_flow/approval_round.rs`.
  - Add `crates/frontend-underwrapd/src/backend_client.rs`.
  - Update the file paths that still exist to reflect their new,
    smaller content.
- Update "Security-critical rules":
  - Drop the "ssh-agent challenge" rules.
  - Add "Never accept a runtime writeback whose env section
    differs from the stored env section."
  - Add "The runtime client never names a storage key — the wire
    speaks `(wrapper_name, resource_name)` only."
- Update the "Planned work" section to clear out the items this
  epic delivers.

### `docs/getting-started.md`

- Drop any mention of per-call approval setup.
- Update the enrollment walkthrough if it references `/v1/status`.

### `examples/config.yaml`

- Drop any wrapper-config field related to approval keys.
- Verify the example still parses against the new schema.

### Acceptance criteria

- [ ] No doc mentions `secret_id`, `/v1/read`, `/v1/write`,
      `/v1/list`, `/v1/read-prefixes`, `/v1/delete`, `/v1/status`,
      `PerCallApproval`, `approval_key`, `ApprovalProof`, or
      `ApprovalRequired` other than as a "this used to exist"
      historical footnote, if at all.
- [ ] The runtime-API documentation matches the three actual
      endpoints exactly, including request / response shapes and
      structured reasons.
- [ ] `CLAUDE.md` reflects the new daemon file layout and the new
      security-critical rules.
- [ ] `examples/config.yaml` parses against the updated daemon.

### Out of scope

- Generated reference docs / RustDoc rebuilds beyond what
  `cargo doc` produces.
- Translating any of the above into other languages.
