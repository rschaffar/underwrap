# Keep failed Secret Service mutations out of live state

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-012
Source themes: C-003

## Outcome

A target-facing Secret Service mutation becomes live in `underwrap-secretsd` only after the host accepts its complete candidate generation and secretsd receives the exact generation-matched `ACK`. If mutation preparation, canonical publication, host validation, or response transport fails, the D-Bus method returns failure without changing the shared in-memory `SecretStore`; later reads and later mutation attempts start from the preceding accepted state rather than observing or republishing rejected content.

The implementation stages one private full-store candidate while retaining the existing store write lock, applies the proposed mutation to that candidate, publishes the candidate, and swaps it into live memory only after exact ACK receipt. This restores the intended failed-operation semantics without changing successful Secret Service behavior, generation framing, host validation, policy, or final writeback.

The F-008 acknowledgement limit remains explicit: a host generation whose ACK was completely written can remain eligible for final writeback even if secretsd fails to receive that ACK or the D-Bus caller never receives success. This task prevents such an unconfirmed candidate from entering secretsd's live memory or contaminating a later attempt; it does not retract host state, prove caller response delivery, or add backend durability.

## Scope

- Add one private mutation transaction boundary inside `underwrap-secretsd`:
  1. acquire and retain the existing `SecretStore` write lock;
  2. clone the accepted live store into one candidate;
  3. apply the complete proposed mutation to the candidate;
  4. canonicalize and publish that candidate through the existing serialized publisher while the write lock remains held;
  5. only after secretsd receives the exact generation-matched ACK, replace live memory with the candidate and return the mutation result.
- A mutation-preparation error or any `publish_store` error leaves the live store structurally equal to its pre-call state. This includes host `REJECT`, request encode/body-bound failure, publication lock poison, write/flush/read failure, malformed or wrong-generation response, EOF, and generation overflow. Preserve existing error classes/messages unless a narrow internal context correction is necessary; do not expose values.
- Route every current state-changing Secret Service handler through the same private transaction boundary:
  - `Service.CreateCollection`;
  - `Service.SetAlias` for both set and clear;
  - `Collection.Delete`;
  - `Collection.CreateItem`, covering both a new random-id item and `replace=true` replacement;
  - the `Collection.Label` setter;
  - `Item.Delete`;
  - `Item.SetSecret`;
  - the `Item.Attributes` setter;
  - the `Item.Label` setter.
- Keep all fallible input validation and path derivation that can precede publication ahead of the commit point. For create handlers, no object interface is registered after a failed candidate publication. Existing post-commit best-effort `ObjectServer::at` warning behavior remains unchanged.
- Preserve successful behavior: an exact ACK commits once; returned ids, replacement flags, object paths, timestamps, aliases, reads, searches, object registration, and metadata diagnostics retain their current contract. Read-only methods, no-op lock/unlock behavior, and initial generation-zero publication are not routed through the mutation helper.
- Preserve publication sequencing. Every complete attempt still consumes its generation under the existing F-008 protocol. A rejected generation can be followed by a valid next generation, but that successor must be built from the last live ACK-observed state and must not contain rejected content.
- Because staging clones secret-bearing state, explicitly zeroize item secret byte buffers in a discarded candidate and in the displaced old store before those buffers are dropped. Use the existing direct `zeroize` dependency of `underwrap-secretsd`; add no dependency and do not move secret values into logs, errors, argv, environment, or ordinary files.
- Add deterministic fault evidence for the production transaction path:
  - every mutator above is exercised under a forced complete host `REJECT` and proves exact pre-call live state remains;
  - create-item new and replacement branches are separate cases;
  - representative wrong-generation/transport-EOF failures also preserve live state;
  - after one explicit rejected attempt, a later ACKed unrelated mutation publishes a whole state that contains the later accepted change but none of the rejected change;
  - successful ACK cases retain current behavior and prove no D-Bus success before exact ACK.
- Tests must cover the production handler/helper routing rather than only reimplementing store mutations in test code. Any responder, task, stream, object server, or bus used by tests is deterministically closed and joined/reaped on success and assertion failure; use bounded synchronization rather than sleeps or port races.
- Synchronize present-state mutation lifecycle and failure semantics in `docs/architecture.md`, `docs/security.md`, `docs/secretsd-security.md`, `docs/gh-libsecret.md`, and `docs/testing.md`. State clone/publish/ACK-observe/commit ordering, side-effect-free live-memory rejection, secret-buffer cleanup, and the retained ACK-response-loss/backend-nondurability limit without ticket/history prose.
- Preserve the deterministic clean-HEAD revalidation artifacts outside the repository:
  - current explicit-defect test log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1110-current-explicit-defect-test.log`, SHA-256 `e5cc0a652248b3b7a3e7fbffb0831eb1fc1365df7fe8b85fa2c77c3d4d8784b5`;
  - desired red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1110-red-before.patch`, SHA-256 `e16fc1d660ee084f7ff704dc648dd754528b995e0c8f5e0fbf864bc2fe4ae2a9`;
  - desired red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1110-red-before.log`, SHA-256 `4a617a8d57d89226763765dea83aea7bf4f135e0eeaa79605c5792150db659c8`;
  - revalidation log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1110-revalidation.log`, SHA-256 `b11cc3306fbbbdc2efa6a403afa1066ade70d3c4bbc79694c6dc31fc1cce3c25`;
  - fresh clean-tree 23-test baseline log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1110-baseline-secretsd.log`, SHA-256 `2edd8a15b1d27a54edcd865ba928b4d7be7eb780ea45d53a8eb05923440e2423`.
  At clean HEAD `f13754f4569268f76f54c6c37e0acbec259c7f1d`, the existing explicit test passes while asserting that `REJECT` and channel loss return `Failed` with label `after` still live. The desired test fails with status 101 because it observes `Some("after")` instead of the required pre-call `Some("Test")`.

Expected implementation paths are limited to the coherent secretsd handler/test/documentation boundary:

- `crates/underwrap-secretsd/src/dbus_service.rs`
- `crates/underwrap-secretsd/src/dbus_service/service.rs`
- `crates/underwrap-secretsd/src/dbus_service/service/tests.rs`
- `crates/underwrap-secretsd/src/dbus_service/collection.rs`
- `crates/underwrap-secretsd/src/dbus_service/collection/tests.rs`
- `crates/underwrap-secretsd/src/dbus_service/item.rs`
- `crates/underwrap-secretsd/src/dbus_service/item/tests.rs`
- an optional sibling-file private mutation module and its `tests.rs` under `crates/underwrap-secretsd/src/dbus_service/`
- `crates/underwrap-secretsd/src/publication.rs` and its sibling tests only if a narrow production-used error/test seam is required
- `docs/architecture.md`
- `docs/security.md`
- `docs/secretsd-security.md`
- `docs/gh-libsecret.md`
- `docs/testing.md`

Any approved behavior-preserving cleanup has its own separately reviewed scope and commit before the finding fix.

## Non-goals

- Do not change the host receiver, publication wire format, generation consumption, strict canonical/semantic validation, 1-MiB publication-body cap, 2-MiB complete-resource cap, ACK-written latest-generation selection, terminal EOF/EAGAIN behavior, or the F-008 ACK-response-loss window.
- Do not claim that D-Bus `Failed` proves an ACK-written host candidate cannot later be selected or that the backend did not commit it. ACK remains invocation-local host retention, not caller response delivery or backend durability.
- Do not add a backend abort/retract verb, two-phase commit, transaction id, revision, compare-and-swap, journal, recovery record, retry protocol, or cross-process rollback. O-004 and O-011 remain separate.
- Do not add handler-level alias-name/target validation, repair aliases/object registration, or change `SetAlias` error classification. Side-effect-free rejection narrows the consequence but does not resolve F-045.
- Do not change wrong-typed `CreateItem` property handling or default invalid variants to a new error; F-046 remains separate.
- Do not update collection `Modified` from item mutations, impose created/modified ordering, future-time checks, or other temporal semantics; F-039 remains separate.
- Do not broaden canonical ingress validation (F-038), error/log redaction policy (F-049), resource/admin mutation atomicity (F-014/F-015), or target-caused denial-of-service handling.
- Do not change object deletion/unregistration semantics, best-effort post-commit object registration, initial generation-zero startup, private bus topology, descriptor ownership, fd 20/fd 21, F-006 gate, F-007 relay/status/liveness, target status, policy/writability, or finalization-error precedence.
- Do not add a dependency, public API/config/wire/storage schema, binary, service, thread, NixOS module change, privileged-chain change, SPA/OpenAPI change, or generated artifact.
- Do not absorb any later medium/low finding, Opportunity, unresolved item, or the medium-to-low transition.

## Acceptance criteria

### Candidate publication and live commit

- One private production helper is the only publication/commit path for all nine current D-Bus mutator handlers; an exact source/call-site inventory confirms no handler still mutates the shared live store before calling `publish_store`.
- The write lock remains held from candidate clone through exact host response and live commit, preserving current mutation serialization and preventing readers from observing a candidate before the response.
- Before exact ACK receipt, only the candidate changes. On mutation-preparation failure or any publication failure, live `SecretStore` equality, item secrets, aliases, labels, attributes, ids, timestamps, and collection/item membership match the pre-call snapshot.
- After exact ACK receipt, the candidate is committed once before D-Bus success. No fallible post-ACK operation that is reported as a D-Bus failure can turn an otherwise rejected method into a hidden live-state commit; create-handler interface registration retains its existing best-effort warning contract.
- A complete `REJECT` followed by an ACKed next attempt consumes consecutive generations, and the second whole-state publication derives from the preceding accepted live state. Rejected label/alias/secret/item/collection content is absent.
- Candidate/displaced secret vectors are explicitly zeroized before drop. Tests and diagnostics contain only synthetic placeholders and lengths/identifiers, never copied secret values.

### Complete mutator failure matrix

- Forced host rejection leaves state unchanged for `CreateCollection`, alias set, alias clear, collection delete, new item create, item replacement, collection label, item delete, item secret, item attributes, and item label.
- Representative malformed/wrong-generation response and channel EOF/write/read failure paths return `org.freedesktop.DBus.Error.Failed` and leave state unchanged. No test claims that an ACK completely written but not received by secretsd is retractable.
- Failed create operations register no new collection/item interface and return no success path. Successful ACKed create and replacement behavior remains covered.
- Existing valid mutation/read/search behavior, idempotent collection creation, random valid item ids, timestamps, alias handling, exact ACK blocking, and publication protocol tests continue to pass.
- Test responders/tasks/streams are closed and joined/reaped deterministically with bounded synchronization. No sleep-only ordering, detached helper, residual process, or generated artifact remains.

### Documentation and regression

- Current-state docs say mutations stage a candidate, publish under the write lock, commit live memory only after secretsd observes exact ACK, and preserve the accepted live store on rejection/failure.
- Docs preserve the distinction between secretsd live-memory commit and host ACK-written selection: ACK loss can still make a caller-observed failure eligible for final writeback, and backend Checkin can still fail later.
- Existing deployed VM evidence still proves legitimate target D-Bus mutation, host publication, policy, Checkin/UsageSignal, later readback, process/descriptor lifecycle, and log hygiene.
- No Cargo manifest/lock, Nix module/test source, protocol/backend/admin/web/OpenAPI path, generated schema/dist/browser output, Python bytecode, or source outside the approved implementation/ticket/handover allowlists changes.
- F-012 is resolved. F-039/F-045/F-046 remain open with updated but non-resolved evidence; F-008/F-009 semantics and every other F-id remain unchanged.

## Security boundary

- Protected asset: the accepted in-memory Secret Service state and the canonical libsecret generation eligible for later backend writeback.
- Attacker: the configured target is trusted with its projection but supplies untrusted D-Bus input for robustness and storage integrity. It can intentionally submit a mutation that host validation rejects and then submit another valid mutation.
- Boundary direction: target D-Bus mutation -> private staged candidate -> canonical publication -> host ACK/REJECT -> secretsd live-memory commit -> eventual lifecycle-gated backend finalization.
- Invariant: without an exact ACK observed by secretsd, the candidate does not enter secretsd live memory or any later candidate derived from that memory. This does not retract a candidate after a host ACK was completely written and preserves the documented response-loss limitation.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2`.

- Preserve and hash the clean-HEAD explicit-defect, desired red-before, and revalidation artifacts recorded above.
- `cargo test -p underwrap-secretsd dbus_service`
- `cargo test -p underwrap-secretsd publication`
- `cargo test -p underwrap-secretsd`
- `cargo test -p underwrap-secretsd-store`
- `cargo test -p underwrap-ns publication`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `nix build .#checks.x86_64-linux.vm-test --print-build-logs --no-link`
- `nix flake check`; if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after coverage tests pass, preserve exact output and do not misattribute it to F-012.
- Static checks for the exact nine-handler/ten-case mutator inventory, no direct live-store mutation-before-publication call site, commit-after-exact-ACK ordering, F-008 response-loss wording, explicit secret-buffer zeroization, docs synchronization, exact staged allowlist, no generated/Python artifacts, `git diff --check`, and staged credential-pattern hygiene.

## Dependencies

- Completed F-008 supplies the private generation protocol, strict host validation, ACK-written selection, and response-loss limit. Completed F-009 supplies independent publication and complete-resource bounds. This task preserves both contracts.
- F-039, F-045, and F-046 remain separately queued C-003 corrections; F-045's rejected-candidate poisoning consequence is narrowed but its validation/error contract remains unresolved.
- F-014/F-015, F-038/F-049, O-004/O-011, and every other named non-goal remain separate.
- No unresolved implementation dependency. Mandatory repository-wide and ticket-local behavior-preserving cleanup discovery begins only after this exact task is approved. No cleanup unit is pre-approved.
