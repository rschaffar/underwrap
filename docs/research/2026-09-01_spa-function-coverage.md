# SPA function-coverage investigation

Status: Release gate accepted, implemented, and directly validated.

Date: 2026-09-01.

Plan owner: [`2026-09-01_release-readiness-plan.md`](../plans/2026-09-01_release-readiness-plan.md).

## Historical input

The optional command was already below its function threshold before the F-058 frontend dependency refresh and remained below it afterward.

On 2026-09-01 the user transferred that unresolved evidence from the completed F-031 through F-058 remediation program into a roadmap without claiming that coverage passed or that it was a release gate.

The active release-readiness plan then promoted that direction into this investigation.

## Fresh reproduction

The reproduction used clean revision `533d05c00d331c0edfe677b3225bd9c4cd64afba` in the dedicated release-readiness worktree.

`npm ci` ran in the pinned Nix development environment with Node 24.19.0 and npm 11.17.0.

It installed 246 packages from the committed lockfile and reported zero vulnerabilities in that installation audit.

The clean-install log has SHA-256 `16263351827a09374579c328ae1da234e0816e0bc40a356010361e637d44da0c`.

`npm run test:coverage` then passed all 29 test files and all 162 tests before returning status 1 for the function threshold alone.

| Metric | Covered | Result | Existing threshold |
|---|---:|---:|---:|
| Statements | 154 of 192 | 80.20% | 80% |
| Branches | 58 of 70 | 82.85% | 80% |
| Functions | 35 of 67 | 52.23% | 80% |
| Lines | 141 of 174 | 81.03% | 80% |

The coverage log has SHA-256 `3c1d5ad8a62579166ee16eb7b27ecbd3c7af31e172b7baff06cca5c6290908d4`.

This fresh result exactly matches the prior recorded totals and confirms that only the function gate is red.

## Uncovered function map

The selected boundary contains the typed API client and the small correctness helpers under `src/lib`.

Every function under `src/lib` already executes in the unit suite.

The meaningful function gap is concentrated in thin endpoint wrappers whose method, schema path, path parameters, query parameters, and body forwarding are observable contracts.

| Module | Uncovered functions | Contract to test |
|---|---|---|
| `src/api/auth.ts` | `fetchBootstrapStatus`, `postPasswordChange` | Exact GET or POST path, password-change body forwarding, and decoded return value. |
| `src/api/client-instances.ts` | List, create, update, and delete wrappers | Exact method and path, `id` interpolation, create or update body forwarding, and decoded return value. |
| `src/api/core.ts` | `api.PUT`; the production-only no-op test reset branch is also unexecuted | Exercise `api.PUT` through a real endpoint wrapper and assert the emitted `Request`; do not add a percentage-only test for the production-only reset branch. |
| `src/api/libsecret.ts` | Tree fetch, item create, collection create/delete, item delete, and alias put/delete | Exact nested collection, item, and alias routes, distinct path-parameter placement, body forwarding, and decoded return value. |
| `src/api/pairs.ts` | Pair create, pair-with-new-resource, and pair delete | Exact store route, alternate create route, wrapper/resource path placement, body forwarding, and decoded return value. |
| `src/api/resources.ts` | Resource create, rename, and delete | Exact method and route, resource-id placement, body forwarding, and decoded return value. |
| `src/api/secrets.ts` | Secrets-view fetch, secret put, and secret rename | Exact view and slot/key routes, rename source-key placement, body forwarding, and decoded return value. |
| `src/api/stores.ts` | Store update | Exact PUT route, store-id placement, body forwarding, and decoded return value. |
| `src/api/users.ts` | User create, update, reset-password, and delete | Exact method and route, user-id placement, request body forwarding, and decoded return value. |
| `src/api/wrapper-permissions.ts` | View fetch, permission put, and permission delete | Exact store/client/wrapper/resource parameter placement, body forwarding, and decoded return value. |

The endpoint wrappers account for 31 executable zero-count functions.

At least 19 additional functions must execute to reach 80 percent, but selectively calling only 19 would leave equivalent request contracts arbitrarily untested.

The recommended test adds table-driven endpoint-contract cases through the real `openapi-fetch` client and a mocked `globalThis.fetch`.

Each case asserts the HTTP method, URL path and query, JSON body when present, credential posture inherited from the client, and returned decoded envelope.

Existing tests already cover CSRF capture and replay, non-2xx `ApiError` mapping, no-content decoding, logout sequencing, and log hygiene.

The new cases therefore cover missing endpoint construction rather than duplicate those cross-cutting tests or add shallow snapshots.

Covering all executable endpoint wrappers should raise function coverage to approximately 98 percent without testing the test-mode-inaccessible production no-op solely for a percentage.

## Enforcement options

The current `web-test` flake check runs the unchanged `npm test` script, while `npm run test:coverage` is optional and absent from `nix flake check`.

The recommended first-release policy is to keep the existing 80-percent thresholds and selected file boundary, add the meaningful endpoint-contract tests, and change the existing `web-test` derivation to run `test:coverage`.

That choice keeps the fast direct `npm test` loop, avoids running the full suite twice in the aggregate flake gate, and makes `nix flake check` enforce the named release threshold.

A weaker alternative is to require the coverage command only in the manual release matrix and documentation.

That alternative can validate one candidate but permits later ordinary flake checks to regress silently.

The final alternative is not to gate the first release on coverage.

That choice requires an explicit user-approved rationale and a retained future owner because the configured command would remain red and optional.

No option requires lowering a threshold, expanding an exclusion, changing a runtime dependency, or changing product behavior.

## Accepted decision and implementation

After reviewing the trade-offs, the user selected the recommended Nix gate on 2026-09-01.

[`ADR-0008`](../adr/0008_spa-unit-coverage-gate.md) owns the lasting policy and its alternatives, consequences, and limits.

`web/src/api/endpoints.test.ts` adds 30 table-driven cases that execute every previously uncovered endpoint wrapper through the real typed client and a mocked fetch boundary.

Each case asserts the HTTP method, origin, path, empty query posture, credentials mode, request body when present, and decoded response envelope.

The focused file passed all 30 tests, with log SHA-256 `4b8b9a39dbd7b72b056e8fad822a71781e043b22ff2c4e2665699096868349df`.

The complete direct coverage run passed all 30 files and 192 tests with 96.35-percent statements, 82.85-percent branches, 98.50-percent functions, and 98.27-percent lines.

That direct coverage log has SHA-256 `0eb465f2a2148f585a8d583233cbcff606adc50c555c3cbcbe999eda99d79d1b`.

The direct TypeScript and production Vite build passed with its existing large-chunk advisory, and its log has SHA-256 `c7ec7c68edd185b2ed65ce29c694a56ba8696aaaffe1bdb337bf730daf032928`.

The first Nix `web-test` probe ran before the new untracked test file entered the flake source and therefore reproduced the old 52.23-percent result.

After the intended files were staged, the exact Nix `web-test` derivation passed all 30 files, all 192 tests, and the same four coverage totals.

The successful Nix derivation was `/nix/store/w6l25k7my7sq8vni7q5i8lkzpxi46x0j-underwrap-web-test-0.1.0.drv`, and its full log has SHA-256 `7dfdf1d4f46c1be32263ecb4a79e69dc7097d29040dd816f07c02e5ad38fa46c`.

`nix flake check --all-systems --no-build` evaluated every supported output with the revised gate and passed with log SHA-256 `2e5ea020c48d4cf9ac5bfcf3893f6767bd78d2ad85190a6b0b52fa18b7df4fbc`.

`flake.nix` now makes the existing `web-test` check run `test:coverage`, so the aggregate `nix flake check` owns enforcement without a duplicate Vitest derivation.

No threshold was lowered, no exclusion was expanded, and the coverage boundary did not change.
