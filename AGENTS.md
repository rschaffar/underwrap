# Agent guidance

Underwrap brokers secrets to trusted CLI targets without giving the untrusted caller the reusable secret.
The supported frontend is the NixOS module and its `underwrap-exec → underwrap-ns` chain; the backend can run on Linux.
The project is pre-release: keep the intended design coherent, retain useful protocol versions, and do not infer stable compatibility or an independent security audit.

## Start with the current owners

- [Contributing](CONTRIBUTING.md): preparation, engineering conventions, and working practices.
- [Architecture](docs/architecture.md): components, identities, and rationale.
- [Security](docs/security.md): threat model, guarantees, preconditions, and limitations.
- [Local protocol](docs/local-protocol.md): privileged handoff, lifetime, projections, and Secret Service validation.
- [Backend](docs/backend.md): resource, policy, API, and persistence semantics.
- [Configuration](docs/configuration.md) and [Operations](docs/operations.md): deployment and operator duties.
- [Testing](docs/testing.md): commands, required checks by boundary, and evidence limits.

Read the relevant owners before editing and read Security before making security judgments.
When an index exists, use CodeGraph first for structural source discovery and blast-radius analysis; verify important test coverage directly.
Do not initialize an index without permission.
Code, generated OpenAPI, and tested Nix options are evidence when prose disagrees; correct the authoritative description rather than changing behavior to fit it.

## Authority and working state

- Stay within the user's requested scope.
  A review or a proposed plan does not authorize implementation.
- Ask before adding dependencies, changing the privileged binary chain, weakening a security boundary, or selecting a materially different product behavior.
- For work needing continuity, use one explicit work owner and a short scope/evidence checklist.
  Temporary working notes do not require a committed plan, ticket, report archive, or completion diary.
- Keep durable decisions and limitations in their maintained topic owner, with present-tense rationale rather than approval transcripts or commit references.
- Preserve unrelated changes; never stash, reset, clean, or overwrite them for convenience.
- Distinguish direct, simulated, unavailable, and user-reported evidence.
  Revalidate evidence when the relevant code or boundary changes.
- Do not turn a test, source observation, or successful command into a broader security, durability, zeroization, or operator-behavior claim.

## Git lifecycle

Use a dedicated feature branch and linked worktree by default, starting from current local `main` unless an existing suitable feature worktree owns the task.
Respect applicable global instructions and explicit user choices about isolation, destination, and integration method; repository defaults do not override those approvals.
The default local destination is `main` through an ordinary `--no-ff` merge.
Keep one writer responsible for repository edits, staging, commits, refs, and worktree lifecycle.
Make coherent local commits after inspecting the exact staged paths and running the relevant checks.
Before integration, present the full verified feature and target IDs, commit summary, checks, and gaps, then obtain approval for that exact state and method.
Do not publish remotely, rewrite history, or remove the feature worktree/branch without separate authority.

## Confidentiality and trust

The caller and other caller-owned non-root processes are untrusted with respect to brokered values.
The configured target is trusted with its projection and can disclose it; target-supplied protocol input remains untrusted for robustness and storage integrity.
The backend, host root, trusted configuration, and kernel are trusted.
For every security concern, name the asset, attacker, boundary direction, invariant, and actual evidence.

Never quote or copy secret values, resource blobs, tokens, credentials, keys, or secret-bearing fixture contents into documentation, reviews, commits, commands, or logs.
Use identifiers and decision metadata only, and remember that operator-selected identifiers can themselves contain sensitive input.
Do not inspect primary live-secret files unless the task explicitly requires it.
Never place secrets in Nix `envSet`, argv, inter-stage environments, ordinary temporary files, or diagnostics.
Use resource slots and the authenticated descriptor channels; zeroize owned buffers where practical without claiming comprehensive erasure.

## Boundaries that must survive every change

- **Persistence:** underwrap-managed durable brokered material uses the backend's selected `StorageBackend`.
  Frontend material stays in memory except invocation-private files on `noswap` tmpfs.
  Libsecret state stays in memory; its bus tmpfs has no `noswap` claim.
  Preserve the separately controlled credential/configuration domains and the documented migration/rotation workspace rules; do not add frontend disk caches or alternate durable paths.
- **Policy:** the backend derives the runtime client instance and store from mTLS.
  Runtime requests name wrappers/resources, never storage keys or `control/*`.
  Recheck permission mode on open and finalize, writability on submitted writeback, and env equality against the loaded snapshot.
  Keep writable local projections even for read-only permissions and preserve whole-blob last-writer-wins limits rather than claiming transactions or CAS.
- **Local entry:** wrapper names are selectors, not identity; reject unknown names and reserved `debug`, ignore `argv[0]`, and let the daemon choose the target after authorization.
  Gate the instance socket on effective GID using hardened `SO_PEERPIDFD`; authenticated-path failures reject without downgrade.
  The PID-based path belongs only to the explicitly weaker posture.
  `allowedUsers` is PATH convenience, not local authorization.
- **Handoff and identity:** authenticate fixed fd 20 as an `AF_UNIX` stream created by the root-provisioned instance daemon before reading fd 3, trusting selectors/argv, or privileged setup.
  Do not accept a bundle-selected descriptor or bearer token substitute.
  Resolve target UID/GID only from the root-owned `(instance, wrapper)` table; missing or invalid authority is fatal, never a fallback to caller or root.
- **Execution:** preserve checked permanent real/effective/saved ID changes, removal of caller supplementary groups, `PR_SET_NO_NEW_PRIVS`, capability clearing/verification, pre-secret non-dumpability, and zero core limits before target exec.
  Keep `env_clear()`, forced paths, the documented allowlist and `LD_*` rejection.
  Close or mark close-on-exec descriptors above stderr; targets and helpers never inherit the resource channel.
- **Lifetime and projection:** preserve namespace/mount/idmap boundaries, complete files planning before effects, exact gate/status/reap requirements, active parent-liveness observation, and suppression on malformed or retained endpoints.
  Keep sole publication endpoint ownership, strict generation validation, ACK-written versus ACK-observed distinctions, and diagnostic-only finalization with target-status preservation.
  Synchronize D-Bus readiness through its address pipe, not socket-path existence.
- **Store lifecycle:** preserve exact-entry/lifecycle-lock revalidation, migration's busy publication and complete-operation lease drainage, and the cancellation-free synchronous post-drain window.
  No registry lock spans waiting, storage I/O, filesystem mutation, or persistence in migration.
  Retain bounded source inventory/fingerprint evidence, one-value copy and one-pair comparison, stable store identity/logical bytes, and verified rollback/cleanup or `recovery-required` disposition.
  Preserve operator quiescence, rotation's distinct non-draining/retention behavior, and absence of secure-media-erasure or power-loss-durability promises.
- **Failure evidence:** check privilege-changing syscall returns, document unsafe pointer/lifetime/fd ownership, and use atomic replacement for persistent writes.
  Preserve documented branch-specific qualifications rather than broadening claims.
  Never remove or weaken a security assertion to make a check pass.

## Mandatory joint review scopes

| Changed boundary | Read and review together |
|---|---|
| Resource schema, HTTP open decoding, env-name validation, secret-bearing cloning/logging, cache or Checkout ownership | `underwrapd/src/backend_client.rs`, `underwrapd/src/wrapper_flow/read.rs`, `underwrapd/src/wrapper_resource_channel.rs`, `underwrap-protocol/src/message/wrapper_resource.rs`, `underwrap-ns/src/resource_channel.rs` |
| Resource schema or writeback | The resource flow above plus `underwrap-backend/src/runtime_api/invocations.rs` |
| Libsecret schema/publication | `underwrap-secretsd/src/publication.rs`, `underwrap-ns/src/publication.rs`, and shared strict validation in `underwrap-protocol/src/resource_blob.rs` |

Paths in this table are relative to `crates/`.
Follow the real flow through callers, framing, rejection, ownership, cancellation, and persistence; a filename list alone is not review evidence.

## Validation and communication

Use focused checks while iterating and every relevant check before committing.
NixOS module, privileged wrapper-chain, identity, capability, namespace, mount, peer-authentication, or deployed-topology changes require the NixOS VM test.
Test-harness changes require the affected harness and integration checks; documentation/example changes require link/path, syntax/evaluation, and affected generated-contract checks.
Follow the [testing matrix](docs/testing.md), including SPA/browser and Rust format/Clippy requirements for affected code.
Use sibling-file Rust test modules and shared `test_support.rs` helpers as described in Contributing; do not introduce inline production `mod tests { ... }` blocks.
Write concise prose with one sentence per line and consistent runtime client instance, resource, wrapper/resource pair, permission row, permission mode, and writable flag terminology.
Report actual changes, checks, unavailable evidence, and remaining obligations without an execution diary.

## Canonical skills and compatibility

`AGENTS.md` is the canonical agent entry point; `CLAUDE.md` must remain its symlink.
`.agents/skills/` owns reusable skills; `.claude/skills` and `.pi/skills` point directly to `../.agents/skills`.
Do not maintain vendor-specific copies.
Read a selected skill before using it:

- `writing-plans`: optional bounded working notes and evidence tracking.
- `reviewing-code`: ordinary scoped read-only review.
- `hardening-code-paths`: authorized hardening in one specific flow.
- `underwrap-codebase-review`: proportionate comprehensive review with independent verification.
