# RF-0005 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `L-01`.
Original review owners: `store-lifecycle.md`, `substantive-inputs.md`, `coordinator-notes.md`, `store-lifecycle-hint-check.py`, `verify-L01-shell-tokenization.log`, `report.md`, `input-ledger.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Path admission: `crates/underwrap-backend/src/admin_api/stores/mod.rs:193-221`; construction: `crates/underwrap-backend/src/admin_api/stores/crud.rs:600-605`; presentation: `web/src/pages/stores/dialogs/cleanup-hint.tsx:34-42`.
Existing `crates/underwrap-backend/src/admin_api/stores/crud/tests.rs:211-249` checks the command prefix and an ordinary temporary path, not shell-special operands.

## Recorded observations

The specialist's synthetic shell check shadowed `rm` with an argument-printing function.
The current unquoted form produced two operands for a space-containing path; a quoted control with `--` produced exactly one matching operand.
Recorded filesystem mutations: 0.
Production handler executed: false.

The successful specialist run exited 0 in 0.037 seconds.
An earlier timing-wrapper invocation exited 127 because its timing utility was absent and did not execute the witness.
The coordinator repeated the harmless check with the same 2-versus-1 result and 0 filesystem mutations; its retained timing line was 0.053 seconds.
Neither run deleted data or called an Underwrap handler.

## Reproduction or verification procedure

Normalized reconstruction, not a command executed during migration:

1. Choose a synthetic absolute path containing one internal space, such as `/synthetic data/store_example`; no such directory needs to exist.
2. Construct the current suggested command as unquoted `rm -rf` followed by that path.
   Construct a control using `rm -rf --` followed by a proper shell-quoted representation of the same path, for example Python `shlex.quote`.
3. Evaluate each only inside a fresh shell where the first statement defines `rm` as an inert function printing its received arguments.
   Do not call external rm, sudo or a real filesystem deletion.
   Capture arguments rather than terminal logs.
4. Exclude only the known option arguments from the count.
   Assert current form yields two nonmatching operands and control yields exactly the original path.
   For future regression coverage, extend safe instrumentation to quotes/metacharacters without permitting command side effects.

## Limits and missing evidence

The executable witness establishes tokenization, not an observed deletion incident or a rendered-browser/real-handler test.
Shell-metacharacter consequences are source/language reasoning beyond the exercised whitespace case.
The procedure is fully reconstructible without the historical external helper.
No new source review, shell witness or repository mutation occurred during migration.

## Resolution evidence

Fixed implementation: `63a68753da4436e33c040dbc01302ac5e3a7f975`, based on local `main` at `c7c3c3d771fedffb1755942e55a219e6c1afb827`.
The following checks ran on the source tree committed unchanged as that implementation, inside the pinned `nix develop` environment on x86_64 Linux with Rust 1.97.1.
The SPA was prepared with `npm --prefix web ci` and `npm --prefix web run build`; backend commands used `UNDERWRAP_WEB_DIST="$PWD/web/dist"`.
Browser commands used the backend built from that tree at `UNDERWRAP_BACKENDD_BINARY="$PWD/target/debug/underwrap-backend"`; temporary state and build/browser output stayed worktree-local.

### Acceptance witnesses

- `crates/underwrap-backend/src/admin_api/stores/crud/cleanup_hint/tests.rs` records shell arguments using an inert `rm` function and disables PATH-based external lookup.
  Ordinary paths, whitespace/controls, quotes, backslashes, substitutions, semicolons, pipes, redirections, wildcard syntax, Unicode and relative paths produce exactly one matching pathname after `-rf --`, with no probe execution or unexpected stderr.
  Negative controls demonstrate the original whitespace split and detect inert command-substitution/extra-command probes.
- The operand oracle preserves raw `file/.`, `symlink/./` and relative dot/dot-dot suffixes rather than using path normalization as its expected value.
  Review caught and removed an intermediate `path::absolute` implementation because it stripped meaningful dot components.
- Handler tests load persisted relative plain and locked encrypted records, reject ordinary deletion, then force-delete only the registry ownership while leaving canonical and retained-generation marker files intact.
  Synthetic non-UTF-8, NUL and empty live paths preserve live and persisted records with a fixed value-free error; a fresh subprocess witnesses the same pre-delete result with an unavailable working directory.
- `web/src/pages/stores.test.tsx` preserves exact response text including quotes, backslash and newline and asserts registry/manual-cleanup/retained-generation/erasure wording.
- `web/e2e/stores.spec.ts` observes a real cleanup response from a disposable backend and compares exact rendered and keyboard-copied command text.
  `web/e2e/cleanup-hint.spec.ts` separately uses an intercepted response to verify exact shell-special/newline rendering and clipboard text without a real DELETE.

### Executed checks

| Command | Observed result |
| --- | --- |
| `cargo test --locked -p underwrap-backend` | Exit 0; 314 library, 3 binary, 4 admin-reference and 8 OpenAPI tests passed; final library run 9.69 seconds |
| `cargo fmt -- --check` | Exit 0 |
| `cargo clippy --locked --all-targets --workspace -- -D warnings` | Exit 0; final run 4.95 seconds |
| `cargo test --locked -p underwrap-backend --test openapi_export --test admin_reference` | Exit 0; 12 checks passed; repeated within final backend suite; no tracked OpenAPI drift |
| `npm --prefix web test` | Exit 0; 38 files / 353 tests passed in 31.20 seconds |
| `npm --prefix web run build` | Exit 0; TypeScript and production bundle passed; existing large-chunk advisory remains |
| `cargo build --locked -p underwrap-backend` | Exit 0; built the current SPA into the fixture backend |
| `(cd web && npm run test:e2e-support)` | Exit 0; fixture support checks passed |
| `(cd web && npm run test:e2e -- --project=desktop-1280 e2e/stores.spec.ts e2e/cleanup-hint.spec.ts --output=../target/browser-final)` | Exit 0; 10 scenarios passed in 15.7 seconds |
| `git diff --cached --check` plus local Markdown destination and three-file/unique-ID inspection | Exit 0; changed documentation links resolved and all 38 finding records retained the required layout |

An initial shell-recorder run failed to locate `sh` because the child's deliberately empty PATH was also used for process lookup; resolving the pinned shell before clearing child PATH corrected the harness.
The first synthetic browser route omitted the DELETE query string and therefore failed to intercept it; matching the pathname plus optional query corrected the witness, with an explicit interception assertion retained.
These were diagnosed test-construction failures, not passing evidence for those earlier attempts.
A read-only independent source review identified the dot-component issue above and found no remaining concrete issue on recheck; it did not independently rerun tests.

### Remaining limits

No real removal command was invoked and no operator incident, production custody, secure erasure, filesystem race exclusion or cross-browser guarantee is established.
The exceptional-path tests use synthetic live paths and an isolated missing working directory, not a production deployment.
Manual trusted-host path verification, retention and key custody remain operator responsibilities described in [Operations](../../../operations.md#decommissioning).
RF-0020's separate wording acceptance is recorded in its [evidence](../RF-0020/evidence.md#resolution-evidence).
