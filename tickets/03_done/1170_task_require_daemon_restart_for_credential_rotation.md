# Require a daemon restart for runtime credential rotation

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-018
Source themes: C-002

## Outcome

Runtime client credential rotation is one explicit operator cutover: successfully generate and install a replacement key/certificate pair on the frontend host, replace that same runtime client instance's registered certificate on the backend, restart the corresponding `underwrapd-<instance>.service`, and verify a wrapper call. The admin UI and current documentation state that order and the expected interruption window instead of claiming every edit takes effect on the next runtime request.

The daemon's current credential lifecycle does not change. A running daemon continues using the `BackendClient` and mTLS identity cached from its existing PEM bytes. Replacing files does not reload it, and an unauthorized response does not clear/rebuild/retry it. Once the backend accepts only the replacement certificate, the still-running daemon fails with enrollment guidance until the explicit restart loads the new pair.

## Scope

- Correct runtime-client edit guidance in the admin SPA:
  - remove the broad page/dialog claim that every edit takes effect on the next runtime request;
  - do not replace it with one grouped timing claim for display name, store mapping, status, and auto-create, whose effects differ; state conditionally only that certificate replacement requires the daemon restart operation;
  - beside the optional certificate field, state the ordered successful rotation operation: run `underwrap-keygen --instance-config /etc/underwrap/daemon/<instance>.yaml` on the frontend, paste and save its public certificate on the existing runtime client instance, run `systemctl restart underwrapd-<instance>.service`, then verify a representative wrapper;
  - state that the existing daemon continues using its cached old identity until restart and that wrapper requests are expected to fail after backend certificate replacement and before restart;
  - do not imply automatic reload, retry, rollback, zero downtime, dual-certificate grace, or that the private key leaves the frontend.
- Extend `web/src/pages/client-instances.test.tsx` so the certificate-replacement dialog pins:
  - the ordered keygen → Save backend certificate → restart service → verify guidance;
  - the expected post-Save/pre-restart interruption;
  - the exact update remains only `{ cert_pem }` for certificate-only rotation;
  - ordinary metadata edits do not acquire misleading credential-restart semantics.
- Add one production NixOS VM regression in `nix/tests/test.py`, using the packaged binaries, generated instance config, real backend admin API, real systemd unit, mTLS, and wrapper chain:
  1. ensure the local wrapper succeeds with output discarded and snapshot the active local client instance's exact fields `id`, `display_name`, `current_store_id`, `current_store_name`, `status`, `auto_create_wrapper_assets`, and `current_credential_fingerprint`, plus one nonzero daemon `MainPID`; exclude `last_seen_at`, which wrapper calls legitimately update;
  2. successfully run `underwrap-keygen --instance-config /etc/underwrap/daemon/local.yaml`, capturing only stdout (the public certificate) in test-controller memory and never reading the private-key file; verify both files have owner **and group** `underwrap-local`, key mode `0400`, and certificate mode `0600`;
  3. before changing backend state, prove the same nonzero daemon PID continues to authenticate with its cached old identity despite replacement files;
  4. authenticated-PUT only `cert_pem` to the same backend client instance via `admin_api`/`_admin_json_body`; re-list and prove only `current_credential_fingerprint` changed among the exact stable fields above;
  5. without restarting, prove the same nonzero PID remains and the next wrapper fails with the existing enrollment guidance because the backend rejects the cached old identity;
  6. restart `underwrapd-local.service`, boundedly wait for a distinct nonzero `MainPID`, and boundedly prove a representative wrapper succeeds with output discarded; do not use stale socket existence or sleeps as readiness evidence;
  7. leave the rotated credential active so later direct-runtime VM checks use the same replacement files and backend record.
  Use finite existing VM-driver bounds/readiness helpers and do not print resource values. Never read the private-key file. Do not interpolate the public certificate into shell source, substitution, argv, or diagnostics: capture keygen stdout only in controller memory and pass `cert_pem` only as an `admin_api` payload through `_admin_json_body`, whose guest file is mode 0600 and removed after use.
- Correct the stale VM-test comment that currently attributes backend-service restart recovery to lazy client reconstruction: the cached reqwest client opens a fresh HTTPS connection because idle pooling is disabled; this is distinct from credential-file reload.
- Synchronize `docs/admin-ui.md`, `docs/getting-started.md`, and `docs/testing.md` with one normative ordered rotation operation, the expected interruption, the VM witness, and current limitations. In the touched getting-started enrollment section, replace the existing `underwrap-keygen` “atomically writes” overclaim with success-path-only wording (“on a successful run, installs …”); this documentation prerequisite neither changes nor resolves F-019's pair-install failure atomicity. Keep `docs/security.md`'s existing cached-identity/restart statement aligned and link rather than duplicate if needed.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1170-credential-restart-red.py`, SHA-256 `2259617390d3f3d35f02aebda3bc26e339b205d34dd9092c27a72ea967c03214`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1170-credential-restart-red.log`, SHA-256 `a69eb34ba2c4973d65e933d4dee39bd6e44f1b95b87342421d76b1d4c76c35c3`;
  - clean SPA baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1170-web-baseline.log`, SHA-256 `9a76e6bc2e2abc3a76cf4340c171df223512ec3a0a865aca8aefb32ebdd71833`;
  - clean VM baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1170-vm-baseline.log`, SHA-256 `327eeeb600b279bc0e85f7f740027f88dd0e6c006a8957ee46df5cb38b50ded3`.
  The script exits 1 with five failures: the page still makes the next-request claim; the edit UI and two operator guides omit the restart command; and the VM lacks a cached-old-failure/restart witness. The clean SPA baseline passes 21 files/102 tests; the clean VM baseline passes.

Expected implementation/product paths are limited to:

- `web/src/pages/client-instances/page.tsx`
- `web/src/pages/client-instances/dialogs/edit.tsx`
- `web/src/pages/client-instances.test.tsx`
- `nix/tests/test.py`
- `docs/admin-ui.md`
- `docs/getting-started.md`
- `docs/testing.md`
- optionally `docs/security.md` only for a concise normative-workflow link/current wording alignment

A narrower product path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Any comment/test-helper simplification must be behavior-preserving, independently reversible, and separately reviewed/validated/committed before the fix.

## Non-goals

- Do not implement live credential reload, file watching, SIGHUP/reload, cached-client invalidation, unauthorized-triggered client reconstruction, automatic retry/replay, or automatic service restart.
- Do not add a backend grace period, credential history, multiple active certificates, rollback endpoint, or zero-downtime/transactional rotation claim.
- Do not change `underwrap-keygen`, credential installation order/atomicity, key/certificate file format, modes, ownership resolution, or error recovery. F-019 owns pair-install failure atomicity; this task tests only a fully successful keygen run, changes only the false atomicity wording required for an accurate operation, and makes no claim about partial failure, prior-pair preservation, or rollback. Record F-019 as still unresolved after that wording correction.
- Do not upload, log, render, or persist the private key outside its existing frontend credential file. Do not expose resource values while testing the wrapper chain. The certificate is public but still travels through the existing request-body path, not argv.
- Do not change backend client-instance handlers, DTOs, fingerprint authority, runtime authentication, daemon request scheduling, wrapper errors, OpenAPI, NixOS module/service definitions, dependencies, or protocol/storage/policy behavior. Do not make one new grouped timing promise for non-credential client-instance edits.
- Do not absorb F-019 or any later finding, F-043, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Operator and UI contract

- The admin page/dialog no longer says certificate replacement takes effect on the next request. It distinguishes metadata updates from credential rotation.
- UI and normative docs state one exact order: successful keygen on frontend → save replacement public cert on same backend instance → restart matching `underwrapd-<instance>.service` → verify wrapper.
- Guidance states that backend replacement invalidates the cached old daemon identity, so wrapper failure before restart is expected. It does not promise reload, retry, rollback, grace, zero downtime, pair-level atomicity, or private-key transfer.
- SPA tests pin the restart command/order/interruption language and retain certificate-only `{ cert_pem }` request evidence.

### Deployed lifecycle evidence

- The VM uses real packaged `underwrap-keygen`, `/etc/underwrap/daemon/local.yaml`, authenticated admin PUT, `underwrapd-local.service`, mTLS, and the setgid wrapper chain.
- Successful keygen installs both files with exact `underwrap-local:underwrap-local` ownership and key `0400` / certificate `0600` modes while the same nonzero daemon PID continues to work from cached old identity before backend cutover.
- Backend certificate replacement changes only `current_credential_fingerprint` among stable `id`, `display_name`, `current_store_id`, `current_store_name`, `status`, and `auto_create_wrapper_assets`; `last_seen_at` is excluded. The same nonzero old PID then fails a wrapper with enrollment guidance and does not self-reload.
- Explicit service restart boundedly yields a distinct nonzero PID, loads the replacement pair, and restores wrapper success. Test commands discard resource output, never read the private key, and never place PEM text in shell/argv/log diagnostics.
- The rotated pair remains active for later VM runtime checks. Backend-service restart evidence is described as fresh connection behavior, not credential reload.

### Scope and separation

- No production Rust/backend/protocol/Nix module/service definition, keygen behavior, credential atomicity, reload/retry/grace, dependency, generated schema, or adjacent finding changes.
- F-018 is resolved as an explicit documented and tested restart-required operation. F-019, live reload, retry/failover, F-043, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: availability of legitimate wrapper requests during an operator-controlled runtime mTLS identity transition, while the private key remains confined to the frontend credential files.
- Actor/fault: a trusted operator successfully installs a replacement pair and registers its public certificate, but a still-running daemon continues presenting the cached old identity; the backend correctly rejects it.
- Boundary direction: frontend credential files + backend client-instance certificate -> cached daemon `BackendClient` identity -> explicit systemd restart -> newly authenticated runtime HTTPS requests.
- Invariant: backend fingerprint policy remains authoritative; only the public certificate crosses to the backend; restart is explicit and verified; no runtime fallback, auto-reload, retry, rollback, or old-certificate grace is inferred.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2` and `PYTHONDONTWRITEBYTECODE=1`.

- Re-hash and preserve the red/baseline artifacts above; run the desired-state script after the fix.
- `cd web && npm test -- src/pages/client-instances.test.tsx`
- `cd web && npm test && npm run build`; remove generated `web/src/api/schema.ts` and `web/dist` afterward.
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static inspection: no next-request credential claim; exact ordered restart guidance; VM marker/field/PID/failure/success/mode assertions; no resource output/private-key/certificate argv; no production Rust/backend/protocol/module/service/keygen/dependency diff; exact staged allowlist; no generated/browser/Python artifacts or residual VM processes.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- Completed F-002 defines first enrollment and the packaged keygen/admin identity surface.
- Completed F-004/F-005 define HTTPS and CA trust; completed F-015 defines persist-before-publish certificate replacement; completed F-017 defines direct async request scheduling without credential reload.
- Existing UI certificate replacement, NixOS service naming/config generation, keygen success path, backend fingerprint replacement, enrollment error mapping, and wrapper-chain tests provide the implementation surface.
- F-019 owns failed pair installation. Live reload, automatic retry/restart, grace credentials, rollback, and all later findings remain separate.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
