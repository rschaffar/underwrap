# ADR-0007: Validate one locked frontend and OpenAPI tool matrix

Status: Accepted

Date: 2026-08-31

## Decision

Treat the committed npm lock and pinned Nix inputs as the supported frontend build, unit-test, browser-test, and OpenAPI client matrix.

Use caret constraints for compatible direct npm dependencies while making no support claim for versions not selected by the committed lock.

Pin `@playwright/test` exactly to the nixpkgs Playwright driver version so package protocol code and browser binaries remain aligned.

Keep TypeScript on the major line accepted by the current openapi-typescript peer contract.

Keep the generated OpenAPI document on its current valid feature line until a format migration has an evidenced product or tooling need.

[`docs/testing.md`](../testing.md) owns the exact selected versions, compatibility holds, health commands, and support limits.

## Alternatives considered

Selecting every latest major independently would ignore peer contracts and the browser-driver coupling that makes the Nix test environment reproducible.

A caret for Playwright could let npm select protocol code that differs from the pinned browser driver.

Pinning every ordinary direct dependency exactly in `package.json` would duplicate the lockfile's selection role without improving the supported-matrix claim.

Treating publication of a newer OpenAPI feature line as an automatic migration requirement would change format without a demonstrated need.

## Consequences

Lock regeneration is maintenance work that must pass peer resolution, audit, outdated review, generated-schema equality, unit tests, production build, and the configured browser suite before the selected matrix changes.

Newer unselected versions are not independently supported.

Playwright driver changes require package and Nix alignment, and TypeScript or OpenAPI feature-line changes require their compatibility evidence rather than version age alone.
