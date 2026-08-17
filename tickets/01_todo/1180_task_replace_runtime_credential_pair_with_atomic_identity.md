# Replace the runtime credential pair with one atomic identity file

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-019
Source themes: C-002

## Outcome

A frontend runtime credential is one private combined PEM identity containing the certificate chain and matching private key. `underwrap-keygen` stages, syncs, owns, modes, and atomically replaces that one file; `underwrapd` reads that one file and gives it directly to reqwest. A failed installation therefore leaves the previous complete usable identity visible instead of publishing one half of a new pair.

This is the user-selected clean first-release design. The pre-release two-path config and keygen CLI are removed rather than retained behind compatibility branches: `backend.client_identity_file` replaces `client_cert_file` + `client_key_file`, and `--client-identity-out` replaces `--client-cert-out` + `--client-key-out`. Keygen still prints only the public certificate to stdout after successful persistence, and the backend still stores/authenticates only that public certificate fingerprint.

## Scope

- Replace the shared daemon backend config shape:
  - remove optional `client_cert_file` and `client_key_file`;
  - add optional `client_identity_file: PathBuf`;
  - omit it only for not-yet-enrolled startup, as today; no partial-pair state exists;
  - update all YAML examples, parsers, fixtures, constructors, and current docs with no legacy aliases or migration branches.
- Replace the keygen explicit CLI/output model:
  - `--client-identity-out <path>` is the sole explicit output flag and conflicts with `--instance-config`;
  - instance-config resolution reads `backend.client_identity_file`;
  - `ResolvedTargets` carries one output path and one optional preferred owner;
  - remove pair-distinctness validation, pair output flags, and pair-specific warning/status text.
- Build the private identity as one zeroizing byte buffer in the same order already accepted by reqwest (certificate PEM followed by private-key PEM). Preserve the public certificate separately only for stdout/backend enrollment. Never print/log the private key or combined identity.
- Atomically install exactly one identity file:
  - target mode is `0400` and owner/group follows the existing explicit daemon-owner → existing-file owner → current process fallback rules;
  - create one temporary file in the target parent, write the full combined identity, apply `fchown` when selected, apply exact mode, `sync_all` the fully prepared file, then perform one `NamedTempFile::persist()` replacement;
  - all fallible staging/metadata work occurs on the temporary file before the single visibility commit;
  - persistence precedes public-certificate stdout delivery. A stdout/write failure after successful persist may leave the new **complete** identity installed even though the caller did not receive the certificate; report that boundary accurately and never describe command-level output delivery as transactional with persistence;
  - discarded candidate buffers are zeroized and failed temporary files are removed by ownership/RAII.
- Add narrowly scoped deterministic install-phase injection used only by sibling unit tests. Cover every pre-commit phase that can return an install error (write, file sync, owner application when selected, mode application, and persist boundary) without adding a production fault hook or dependency.
  - Seed one complete old combined identity and snapshot bytes, uid/gid, mode, and directory entries.
  - At each injected **pre-commit install** failure, require an error, byte/metadata-identical old target, no new identity visibility, and no residual staging file.
  - On success, require one complete new combined identity with exact metadata.
  - State the exact commit point and keep power-loss/parent-directory-fsync durability outside the claim.
- Replace underwrapd credential admission and client construction:
  - config validation no longer checks two-option parity;
  - lazy enrollment checks only the one identity path;
  - startup permission validation checks the identity file once, follows the existing owner and `0600`-or-tighter/parent-`0700` rules, and emits identity-specific errors;
  - `BackendClient::open` validates URL before reading the identity, reads it once, and passes it directly to `reqwest::Identity::from_pem` while preserving HTTPS-only, exclusive configured CA/default WebPKI, disabled idle pooling, admission/deadline, and typed errors.
- Update NixOS first-release layout:
  - generated daemon config uses `/var/lib/underwrap-<instance>/client-<instance>.identity.pem` as `client_identity_file`;
  - keygen helper documentation/links use that config or the one explicit flag;
  - service read permissions and state-dir hardening remain unchanged;
  - VM fixture identities combine the existing public test certificate and private test key into one mode-`0400` test-only identity file. Production key material never enters the Nix store.
- Adapt integration/test PKI helpers so daemon configs receive one combined identity while tests that intentionally exercise independent server/client certificate primitives may retain separate public/private fixture source files. Do not merge or rewrite fixture source PEMs merely for aesthetics.
- Update the completed F-018 cutover witness and docs to one identity file:
  - VM keygen success checks one `underwrap-local:underwrap-local` mode-`0400` identity path;
  - direct runtime curl may pass that same combined PEM path to both `--cert` and `--key`, proving deployed tooling accepts it;
  - preserve keygen → backend public-cert PUT → same-PID cached-old failure → restart → distinct-PID success and all no-output/no-private-read guarantees.
- Synchronize current `README.md`, relevant examples, and `docs/architecture.md`, `docs/configuration.md`, `docs/getting-started.md`, `docs/admin-ui.md`, `docs/remote-backend.md`, `docs/security.md`, and `docs/testing.md`. Use one normative explanation and remove pair-atomicity/rollback wording. Say atomic replacement visibility after file sync, not power-loss durability.
- Preserve clean-HEAD evidence outside the repository:
  - red-before patch `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1180-red-before.patch`, SHA-256 `b96fb9351d6b767b40dfae10841e07048b0a2b3148627fa1529cfd080975d4d1`;
  - red-before log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1180-red-before.log`, SHA-256 `35c36038b0e7133a887009bdf98667fdadd625966b2016bf2aabd7cf062c5c38`;
  - clean keygen baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1180-keygen-baseline.log`, SHA-256 `0d847b1615be92c48648e3c671de196feca45ca009650e91dc17e48816bf676d`.
  The deterministic red test exits 101: after key replacement succeeds and certificate staging is blocked by a regular-file parent, the restored state is `NEW_KEY` + `OLD_CERT` instead of the required old matched pair. The clean baseline passes 10 keygen library tests and its binary/doc targets.

Expected implementation/product paths are limited to the coherent combined-identity migration:

- `crates/underwrap-protocol/src/config.rs` and config tests
- `crates/underwrap-keygen/src/lib.rs` and sibling tests
- `crates/underwrapd/src/backend_client.rs`, `server.rs`, `startup.rs`, config/test support, and sibling tests
- directly affected integration harness/PKI and daemon/backend-CA tests
- `nix/modules/underwrap.nix`, `nix/tests/node.nix`, and `nix/tests/test.py`
- `examples/daemon-config.yaml` and directly affected examples
- `README.md` and the current docs named above

A narrower path set is preferred if it completes the no-compatibility migration. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Test-fixture/helper simplifications qualify only when useful before and after the combined-identity fix.

## Non-goals

- Do not keep deprecated config fields, dual-read fallback, old keygen flags, two-file output, migration tooling, or compatibility aliases. This greenfield first release has no deployed credential-path surface.
- Do not add journal/rollback/recovery state, versioned generation directories, symlink swapping, directory-fd snapshot logic, a second public-certificate file, multiple active credentials, backend grace, or live daemon reload/retry. Those alternatives were rejected in favor of the explicit user-selected single identity file.
- Do not claim parent-directory fsync, power-loss durability, secure media erasure, transactional stdout delivery/backend enrollment, or atomicity across frontend persistence and the later backend public-certificate PUT. The guarantee is one-file atomic replacement visibility after fully prepared temporary-file sync. A stdout failure after commit leaves a complete new identity, not the old identity or a half-pair.
- Do not change certificate generation/identity semantics, backend fingerprint policy, runtime API/admin API, public certificate stdout shape, F-018 explicit restart operation, request scheduling, keygen owner resolution precedence, systemd hardening, wrapper chain, storage/policy, or secret projection behavior.
- Do not add dependencies, expose key/identity bytes in logs/argv/stdout, inspect live secrets, or put production private material in Nix derivations.
- Do not absorb F-020 or any later finding, F-043, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### One atomic credential authority

- The repository contains exactly one daemon credential-path field and one keygen output flag: `client_identity_file` / `--client-identity-out`. Old two-file names have zero production/config/doc references other than explicit red/history evidence outside current source.
- A generated identity file contains the complete matching certificate and private key, is mode `0400`, has the selected owner/group, and is accepted by reqwest and deployed curl usage.
- Keygen stdout remains exactly the public certificate after successful persist; stderr/logs name identifiers/paths only and never contain private or combined identity bytes.
- Existing identity owner fallback and URL-before-credential-read semantics remain exact.

### Failure atomicity

- One temporary file receives all bytes, sync, owner, and mode work before exactly one atomic persist. There is no second credential visibility commit.
- Deterministic failures at every injected pre-commit install phase preserve the previous target's bytes, uid/gid, mode, and usability and leave no staging file. Success publishes the complete new identity with exact metadata. Separately pin/document the post-commit stdout boundary without promising rollback.
- Tests and docs identify persist as the visibility commit and do not claim parent-directory/power-loss durability, transactional stdout delivery, cross-host transactionality, rollback, or secure erasure.
- The preserved two-file red evidence is structurally impossible in the resulting design and equivalent desired-state tests pass.

### End-to-end migration and scope

- Protocol/YAML, keygen CLI, daemon startup/client, Nix module/VM, integration helpers, examples, and current docs use only the combined identity model; no compatibility branch remains.
- F-018's production VM cutover still passes with one identity file, exact mode/owner, same-PID cached-old failure, explicit restart, distinct-PID success, and later direct mTLS requests.
- No backend/API/fingerprint policy, live reload/retry/grace, dependency, privileged-chain, secret projection, or adjacent finding change.
- F-019 is resolved. F-020+, F-043, power-loss durability, backend/frontend transactionality, and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: a matched, parseable, reusable frontend runtime mTLS identity and confidentiality of its private-key portion.
- Actor/fault: a trusted operator invokes keygen while filesystem staging/metadata/persist operations may fail; an untrusted local process must not learn the identity bytes. Host root remains trusted.
- Boundary direction: generated certificate+private key -> zeroizing combined buffer -> mode-`0400` same-directory temporary file -> one atomic persist -> underwrapd/reqwest identity parser -> backend fingerprint authentication.
- Invariant: before the visibility commit, readers see the complete old identity; after successful commit, readers see the complete new identity; a reported pre-commit failure publishes neither a half-pair nor a new identity.

## Validation

Run resource-intensive checks sequentially with `CARGO_BUILD_JOBS=2` and `PYTHONDONTWRITEBYTECODE=1`.

- Re-hash and preserve the red/baseline artifacts above.
- Focused keygen phase/failure/success/CLI tests.
- Focused protocol config, underwrapd startup/backend-client/URL/CA/redirect, and integration daemon/backend-CA tests.
- Repository static inventory: zero old field/flag/current-doc references; exactly one identity path; one persist; no second private/public output file; no compatibility branch; no key/identity logging.
- `cargo test -p underwrap-keygen`
- `cargo test -p underwrap-protocol`
- `cargo test -p underwrapd`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cd web && npm test && npm run build`; remove generated schema/dist afterward.
- Exact staged allowlist and no generated/browser/Python/worktree/process artifacts.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- User explicitly selected **one combined identity PEM** over two-file journal/rollback and versioned-directory pointer alternatives after the D-111 architecture stop.
- Completed F-004/F-005 define URL/HTTPS/CA behavior; F-017 defines async scheduling; F-018 defines explicit restart cutover. All remain semantically unchanged while their config/test paths migrate.
- F-019's prior prose overclaim was narrowed by F-018, but the sequential two-file implementation and deterministic mismatch remained until this task.
- No unresolved implementation dependency. Cleanup discovery begins automatically after materialization; no cleanup unit is pre-approved.
