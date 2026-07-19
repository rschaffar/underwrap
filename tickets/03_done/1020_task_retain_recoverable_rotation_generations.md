# Retain recoverable generations during encrypted-store key rotation

Parent: none
Source review: tickets/_review/codebase_2026-07-18.md
Source findings: F-003
Source themes: C-001

## Outcome

Encrypted-store key rotation never intentionally makes the operator's last
externally held master key useless. Every complete generation displaced by a
successful first rename remains a uniquely named backend-host recovery artifact
across later handler actions, returned I/O errors, checked rollback failure,
process-visible interruption/restart, later rotations, and loss of the one-time
replacement-key response. A successful rollback instead restores that same old
generation as the canonical live generation. Successful rotation activates a
freshly encrypted generation and returns its new key once; an authorized host
operator removes a selected retained generation only after independently
verifying a later key by lock/restart/unlock plus canary and representative data
reads.

This corrects the destructive rotation ordering without persisting keys or
claiming that HTTP delivery or a browser acknowledgement proves external key
custody. It guarantees process-visible recoverability, not survival from
arbitrary storage corruption, media loss, or unflushed power loss.

## Scope

- Acquire the existing per-store rotation mutex before resolving the
  authoritative encrypted/unlocked store record, active backend, and data path.
  A queued rotation must use the generation active when it is admitted, not a
  backend captured before waiting.
- Create each staging directory exclusively under a server-generated,
  collision-safe sibling name with secure permissions. Pair it with a unique,
  recognizable recovery name derived only from the trusted store id and the
  same rotation id, for example:
  `.<store-id>.underwrap-rotation-{stage,backup}-<rotation-id>`.
- Stage and verify a complete new generation before moving the live directory.
  Pre-swap failures may remove only the incomplete stage; they leave the old
  live generation and registry backend unchanged.
- Once the first rename has displaced the old live directory, that directory is
  a recovery generation and rotation never deletes, overwrites, or modifies it.
  Promotion or promoted-generation open/canary failure attempts checked
  rollback. Successful rollback restores the old canonical directory and keeps
  the old backend usable. If rollback cannot restore the canonical directory,
  transition the existing registry entry to `StoreState::Locked` before
  returning a stable non-secret `rotation_recovery_required` error. No returned
  error may leave an unlocked backend whose key/path disagrees with the
  canonical directory; preserve the old recovery generation and every other
  potentially useful directory.
- Reopen and verify the promoted generation before publishing its new in-memory
  backend. Rotation does not change `StoreRecord`, so remove the redundant
  record reassignment/save and prove that rotation performs no registry-file
  write.
- Retain every displaced generation across success, response serialization or
  delivery loss, restart, retries, and later rotations. Later rotations create
  distinct artifacts and never consume earlier ones. Retention is unbounded
  until a trusted backend-host operator deliberately cleans selected artifacts.
- Encrypted-store creation retains explicit create-new semantics. Unlock and
  post-promotion reopen use open-existing semantics and reject a missing
  canonical directory with a stable recovery-required error without creating
  it. Startup remains the existing encrypted-store `Locked` behavior: it does
  not enumerate, select, promote, or reconcile recovery generations.
- Preserve the current one-request API and `key_hex` response shape. Update
  endpoint/OpenAPI wording, the SPA rotation/reveal guidance, tests, and
  current-state documentation to explain backend-host recovery retention,
  manual verification/cleanup, disk growth, stale-ciphertext exposure, and the
  fact that UI acknowledgement is local only.
- Add deterministic filesystem/failpoint, real-route response-discard,
  repeated/overlapping rotation, restart/manual-recovery, and secret-hygiene
  regressions. Keep ordinary store traffic under the existing operator
  quiescence requirement.

## Non-goals

- Do not add Opportunity O-005's acknowledged rotation lifecycle: no pending-
  custody state, manifest, confirmation/finalize/abort endpoint, server-side
  custody bit, browser-to-server acknowledgement, startup reconciliation,
  automatic generation selection, or automatic cleanup.
- Do not add Opportunity O-003 power-loss/fsync durability semantics. The fix
  covers returned I/O errors, process-visible rename states, restart, and
  transport loss; it does not claim persistence across unflushed storage or
  sudden power loss.
- Do not add Opportunity O-001's versioned envelope or logical-key AEAD AAD, and
  do not change the encrypted record format.
- Do not add Opportunity O-004 revisions/CAS or coordinate ordinary runtime and
  admin reads/writes. Rotation still requires all store access to be quiesced.
- Do not persist, escrow, hash, or log a master key; accept an operator-supplied
  replacement key; add TPM/HSM/vault integration; or add a dependency or
  configuration option.
- Do not expose recovery paths or a cleanup operation through the admin/runtime
  API. Recovery and cleanup use trusted backend-host filesystem authority and
  documented server-derived names.
- Do not absorb general registry atomicity F-015, resource deletion F-011,
  store CRUD/force-delete redesign, registry-backed recovery generations, API
  cleanup, OpenAPI F-016, encrypted-store creation redesign, or a general
  backup/restore product. Existing store deletion does not automatically
  discover or delete retained sibling artifacts; current-state operator
  documentation must state that those backend-host files require separate
  explicit enumeration and cleanup.
- Do not change the production NixOS frontend module, runtime transport,
  `underwrapd`, or the `underwrap-exec → underwrap-ns` chain.

## Acceptance criteria

- On successful rotation, the active generation contains the complete canary,
  resource blobs, and control documents under a fresh 32-byte key, while the
  complete pre-rotation generation remains at a unique `0700` backend-host
  recovery path and is decryptable with the previously held key.
- The handler never calls `remove_dir_all` (or equivalent destructive cleanup)
  on a directory that became a recovery generation. Each later rotation retains
  another unique pre-rotation snapshot that rotation never modifies, overwrites,
  or deletes; no retry or later success consumes an earlier artifact.
- Rotation acquires the per-store mutex and then re-resolves the current record,
  state, backend, and directory. Deterministic overlapping calls use successive
  admitted generations rather than a stale backend captured before the lock.
- Stage creation is exclusive and collision-safe. Existing files, directories,
  or symlinks at a proposed stage/backup name are never reused or overwritten;
  generated names contain no key, key hash, plaintext, resource name, or secret
  value.
- Stage-create/open, source-list/read, canary/destination-write, and first-rename
  failures leave the old live generation and in-memory registry usable with the
  old key. Only an incomplete stage may be removed.
- Promotion failure with successful rollback restores the old live directory
  and usable old backend. Promotion or rollback failure that cannot restore the
  canonical path retains the old-key-decryptable recovery directory, preserves
  every potentially useful generation, transitions the existing entry to
  `StoreState::Locked`, and returns `rotation_recovery_required` without key or
  plaintext data. No new persisted or third lifecycle state is introduced.
- Promoted-backend open/canary verification failure follows the same checked
  rollback rules. The new backend is installed only after successful open and
  verification; a returned error never leaves the registry claiming an unlocked
  backend/key that disagrees with the canonical live path.
- Rotation neither mutates nor saves the byte-identical `StoreRecord`. An
  unusable registry directory after initial setup does not introduce a
  post-commit rotation failure, and persisted registry bytes remain unchanged.
- An authenticated admin request through the real HTTP router completes
  rotation, and the test client then drops the response/body before `key_hex` is
  parsed, copied, or retained. Server completion is established from backend
  state/artifact evidence; the pre-rotation key—not the replacement key—decrypts
  the retained canary plus seeded resource and control data. A separate hygiene
  assertion proves the replacement key is absent from persistent backend state,
  logs, errors, audit fields, browser storage, cookies, URLs, and later GET/list
  responses. The test makes no TCP-delivery or power-loss claim.
- At least three sequential rotations retain distinct complete snapshots. Every
  captured prior key decrypts its corresponding retained generation, including
  when an intermediate response is deliberately ignored.
- Unlock and rotated-backend reopen reject a missing canonical data directory
  without creating it. Startup retains the existing encrypted-store `Locked`
  state and performs no artifact discovery or reconciliation. Restart tests
  construct each process-visible interrupted-rename state, assert all existing
  artifacts remain byte-for-byte untouched, and assert unlock does not recreate
  a missing canonical directory.
- `nix/tests/test.py`'s `encrypted_store_in_memory_keys()` deployed VM scenario
  performs a real backend-host recovery drill: stop/quiesce the backend,
  preserve the current live generation
  under a fresh non-overwriting sibling name, promote one selected retained
  generation, restart, unlock with its already held key, and verify the canary
  plus representative seeded resource/control data. A cleanup drill removes
  only an explicitly selected artifact after a later key has been verified by a
  separate lock/restart/unlock and representative reads; active and other
  retained generations remain unchanged.
- SPA pre-rotation and key-reveal copy state that old ciphertext is retained on
  the backend host, consumes disk, remains decryptable with the old key, and
  must be removed manually only after replacement-key verification. The local
  acknowledgement remains non-dismissible UI protection, not a custody claim.
  `web/e2e/stores.spec.ts`'s `rotate-key flow surfaces a fresh key once for an
  existing encrypted store` Playwright test asserts this guidance and continued
  absence of key material from browser storage, cookies, and URLs.
- Documentation gives the exact naming/enumeration rule and a non-overwriting,
  quiesced offline recovery/verification/cleanup procedure. It preserves the
  warning that ordinary reads/writes are not coordinated and does not claim
  transactionality, response delivery, CAS, or power-loss durability.
- Success and every injected failure are audited with safe store/phase/result
  identifiers only. Tests assert that old/new key hex, seeded plaintext,
  decrypted blobs, and contained secret paths do not enter logs, errors,
  registry metadata, recovery names, or browser persistence.
- Existing plain/locked refusal, successful rotation, one-time key reveal,
  store-list non-disclosure, CSRF/session, full workspace, browser, and deployed
  VM behavior remain green.

## Security boundary

- Protected asset: availability and integrity of every encrypted resource and
  control document, plus continued usefulness of at least one master key
  already held outside underwrap.
- Attacker/failure: no malicious principal is required; filesystem errors,
  process interruption, backend restart, client/proxy disconnect, or a lost HTTP
  response can cross the boundary. A trusted host operator may also clean the
  wrong artifact accidentally; host root and backend storage remain in the
  trusted computing base.
- Boundary direction: backend ciphertext-generation commit → one-time
  replacement-key transport → external operator custody. Commit must not make
  all externally held keys useless.
- Invariant: `AGENTS.md` requires persistent security mechanisms to fail closed
  and `docs/security.md` assigns encrypted-store key custody to the operator.
  Until a later key is independently verified, one externally held key must
  decrypt one complete retained generation. Retention trades delayed old-key
  revocation and disk use for recoverability; deletion is an explicit trusted
  host action, never an inference from response success.

## Validation

- `cargo test -p underwrap-backend-store encrypted`
- `cargo test -p underwrap-backend admin_api::stores::lifecycle`
- `cargo test -p underwrap-backend`
- `cargo test -p integration-tests rotation`
- `cargo test -p integration-tests`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`
- `nix develop --command bash -lc 'cd web && npm run test:e2e'`
- `nix build .#checks.x86_64-linux.vm-test`
- focused key/log/browser-storage hygiene and `git diff --check`

## Dependencies

- `tickets/03_done/1000_task_preserve_untouched_resource_slots.md` — satisfied;
  F-001 established non-destructive typed resource mutation before this
  encrypted-generation lifecycle correction.
