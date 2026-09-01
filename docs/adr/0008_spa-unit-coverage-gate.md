# ADR-0008: Gate the selected SPA unit boundary at 80 percent

Status: Accepted

Date: 2026-09-01

## Decision

Treat `npm run test:coverage` as a first-release and continuing regression gate.

Keep the selected boundary at hand-written `src/api/**/*.ts` and `src/lib/**/*.ts`, excluding generated API types and test files.

Keep the existing 80-percent thresholds for statements, branches, functions, and lines.

Make the existing Nix `web-test` check run `test:coverage` so `nix flake check` enforces the gate without running the complete Vitest suite twice.

Require tests added for this boundary to assert observable behavior such as request construction, response decoding, error mapping, URL handling, or helper results rather than invoking functions solely to change a percentage.

Keep React Testing Library component evidence and Playwright browser evidence in their existing layers instead of broadening this unit boundary to duplicate them.

Treat coverage as a regression signal rather than a security proof, correctness proof, or substitute for review and end-to-end validation.

[`docs/testing.md`](../testing.md) owns the direct commands and aggregate-check behavior.

## Alternatives considered

Keeping coverage only in the manual release matrix would validate one candidate but allow ordinary flake checks to accept a later regression.

Leaving the command advisory would retain a configured red threshold with no enforcement owner.

Adding a second flake coverage derivation would run the complete Vitest suite twice without producing distinct behavioral evidence.

Lowering a threshold or expanding exclusions would hide the existing gap rather than test the selected correctness boundary.

Including all components in the unit threshold would incentivize shallow rendering and snapshot tests that duplicate the component and browser layers.

## Consequences

A regression below any selected 80-percent threshold fails `web-test` and therefore fails `nix flake check`.

The direct `npm test` command remains the fast complete-suite loop without coverage instrumentation.

Changes to the selected boundary, thresholds, exclusions, coverage provider, or enforcement point are policy changes that must update this ADR and their evidence.

Pinned frontend tooling limits incidental coverage drift, but a toolchain refresh must still review changed instrumentation rather than mechanically add tests or weaken the gate.

The production-only no-op branch of the test reset hook remains unexecuted because testing it only to reach 100 percent would not add a product contract.
