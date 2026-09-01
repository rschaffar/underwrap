# SPA unit coverage

Status: Future direction.

## Current evidence

The optional `npm run test:coverage` command runs the complete SPA unit suite and then applies the V8 coverage gates configured in `web/vite.config.ts`.

The current boundary includes `src/api/**/*.ts` and `src/lib/**/*.ts`, excludes `src/api/types/**` and test files, and requires 80 percent for statements, branches, functions, and lines.

A direct run in the pinned Nix development environment on 2026-09-01 passed all 29 test files and 162 tests before returning nonzero because function coverage was 52.23 percent (35 of 67), below the 80 percent threshold.

The same run measured 80.20 percent statements, 82.85 percent branches, and 81.03 percent lines, so only the function threshold was red.

The command was already below threshold before the F-058 frontend dependency refresh and remains below threshold afterward, so the refresh does not own or mask this gap.

## Direction

Add meaningful unit coverage for the selected API and library functions until the existing command passes without reducing any threshold or expanding any exclusion.

Prefer assertions over request construction, response decoding, error mapping, URL handling, and other observable contracts rather than shallow snapshots or calls made only to increase a percentage.

Keep component and end-to-end evidence in their existing React Testing Library and Playwright layers instead of broadening this unit gate merely to duplicate those layers.

Promote this direction into a grounded active plan before implementation, with exact test targets, behavior claims, validation, and review scope.

This roadmap grants no implementation authority and implies no product behavior change.
