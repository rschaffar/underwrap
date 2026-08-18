# Revoke the server session before reporting sign-out success

Parent: none
Source review: `tickets/_review/codebase_2026-07-18.md`
Source findings: F-021
Source themes: C-005

## Outcome

Visiting `/logout` in a fresh browser document first obtains a CSRF synchronizer token from the tolerant `whoami` GET, then performs the logout POST. The SPA clears cached admin data and redirects to `/login` only after the backend confirms session destruction. Network, CSRF, or backend failure remains visibly on `/logout` with a Retry action; it is never misreported as successful sign-out.

This aligns hard navigation with in-app sign-out without changing the backend session, cookie, CSRF, or logout handler contract. An absent session is already represented by tolerant `whoami`; the authenticated logout POST remains the authority-clearing operation.

## Scope

- Add one typed auth helper in `web/src/api/auth.ts`, exported through the existing client barrel, that always performs in order:
  1. `GET /v1/admin/auth/whoami` to receive/capture a fresh `X-CSRF-Token` through the existing middleware, regardless of whether module state was empty;
  2. only after that GET succeeds, `POST /v1/admin/auth/logout` through the normal typed client.
  Do not read the HttpOnly CSRF cookie, bypass middleware, manually copy tokens, or log bodies/tokens.
- Replace `/logout` route's swallowing `beforeLoad` redirect in `web/src/router.tsx` with an explicit route component lifecycle:
  - make at most one initial revocation attempt even under React StrictMode's effect replay;
  - show **Signing out…** while the CSRF-prime + logout sequence is in flight;
  - after successful logout POST, clear the shared React Query cache and replace-navigate to `/login`;
  - on prime or logout failure, do **not** clear cached data, do **not** redirect, and show a non-sensitive **Sign out failed** state with the existing coarse error mapping and a **Retry sign out** button;
  - Retry re-runs the complete prime + POST sequence and cannot overlap another attempt;
  - confirmation of backend revocation, not local cache clearing, defines sign-out success.
- Add focused client tests in `web/src/api/client.test.ts` that start with an empty module-scoped token and prove the helper sends exactly a whoami GET followed by logout POST, with the token captured from the GET response attached to the POST. Pin failure ordering: a failed prime sends no POST, and a failed POST is propagated rather than swallowed.
- Update `web/e2e/auth-flows.spec.ts`:
  - change the server-side invalidation scenario to use a real `page.goto(<base>/logout)` fresh document, retain/replay the old session cookie, and prove a protected endpoint returns canonical 401 after the route reaches `/login`;
  - preserve the in-app Link CSRF-header scenario;
  - add **`failed hard-navigation logout remains visible and retryable`**: intercept only the first logout POST with a structured 500, prove the POST carried a non-empty CSRF token, `/logout` shows failure/Retry instead of `/login`, and the still-valid session reaches a protected endpoint; then Retry reaches the real backend logout, redirects to login, and the captured old session cookie is rejected;
  - use request/response synchronization rather than sleeps and clean up any extra request context.
- Synchronize concise current-state guidance in `docs/admin-ui.md`, `docs/security.md`, and `docs/testing.md`: sign-out primes CSRF even on hard navigation; success means backend session destruction; failure is visible/retryable and leaves server authority/cache intact; E2E covers success, failed-first-attempt, retry, and stale-cookie rejection.
- Preserve clean-HEAD evidence outside the repository:
  - desired-state script `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1200-logout-red.py`, SHA-256 `f1b3e6bb8945ed1e72a7c27bfc64cad2eb7426d10f79be0a7ad8ff6295e6d6d0`;
  - red log `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1200-logout-red.log`, SHA-256 `f67e0883d56b04767fdb192ddfb2fbe442a56c382094a1f33774607251a61817`;
  - clean focused unit baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1200-web-baseline.log`, SHA-256 `72810333c281f27bc0441a8d2721c9c29194df4abfd26c5c6e2c735b7e584c32`;
  - clean auth E2E baseline `/tmp/underwrap-codebase-review-2026-07-18-WJidbr/1200-e2e-baseline.log`, SHA-256 `56d221f84c8504b76c3c1aa3ad1abd2cdc4e1e8edc41c2f2823f382e2c26eae3`.
  The script exits 1 with all six desired conditions absent. Focused client/router unit baseline passes 16 tests; existing auth E2E passes five tests but signs out only through the already-primed in-app Link.

Expected implementation/product paths are limited to:

- `web/src/api/auth.ts`
- `web/src/api/client.test.ts`
- `web/src/router.tsx`
- `web/e2e/auth-flows.spec.ts`
- `docs/admin-ui.md`
- `docs/security.md`
- `docs/testing.md`

A narrower path set is preferred if it satisfies every criterion. Required board moves and `tickets/_handover/codebase_2026-07-18.md` updates are workflow state, not product-scope expansion. Mandatory fresh cleanup discovery runs first; no cleanup is pre-approved. Router/component extraction is cleanup only if it reduces current complexity independently of the sign-out fix.

## Non-goals

- Do not change backend logout/whoami handlers, session storage, cookie attributes/names, CSRF middleware/config, OpenAPI, generated schema, session lifetime, login/bootstrap routing, or protected-route behavior.
- Do not make logout a GET, read HttpOnly cookies from JavaScript, manually construct CSRF headers, accept local cache clearing as revocation, swallow failures, or automatically retry in a loop.
- Do not clear query cache or show `/login` on failed revocation. Do not display server internals, tokens, cookies, or session values in the error UI/tests/logs.
- Do not add dependencies, global state libraries, service workers, background logout, cross-tab coordination, or local/session storage.
- Do not absorb F-022 or any later finding, Opportunity, unresolved item, migration, or severity transition.

## Acceptance criteria

### Revocation sequence and UI

- From an empty CSRF module state, one hard `/logout` visit issues whoami GET then logout POST with the response token, never POST-before-prime.
- Exactly one initial attempt runs under StrictMode. Success clears cache only after POST 204 and replace-navigates to login.
- Prime/POST failure remains on `/logout`, retains cache/session authority, surfaces a non-sensitive failure, and exposes one non-overlapping Retry. Retry repeats GET→POST and redirects only after success.

### Evidence

- Unit tests pin request order/token replay/no-POST-on-prime-failure/POST-failure propagation.
- E2E hard-navigation success replays the old cookie and receives protected 401. The failed-first-attempt test proves visible/retryable failure, non-empty CSRF header, still-authenticated protected access before retry, successful real logout after retry, and old-cookie rejection.
- Existing in-app sign-out and unauthenticated/login/bootstrap flows remain green.

### Documentation and scope

- Current docs define backend revocation as success and name hard-navigation/retry evidence without overclaiming cache or cookie behavior.
- No backend/API/schema/cookie/CSRF/session-lifetime/dependency/generated-output or adjacent finding changes.
- F-021 is resolved. F-022+ and every other finding/Opportunity remain separately queued.

## Security boundary

- Protected asset: the opaque authenticated admin session and its authority over protected backend operations.
- Attacker: a later user of the same browser profile can reuse a still-valid HttpOnly session cookie if the SPA falsely reports sign-out after a failed POST.
- Boundary direction: fresh `/logout` document -> CSRF prime -> authenticated logout POST -> backend session destruction/cookie clearing -> local cache clear/login navigation.
- Invariant: sign-out success is shown only after backend revocation succeeds; failure leaves authority visibly unresolved and retryable.

## Validation

- Re-hash and preserve the red/baseline artifacts above; run the desired-state script after the fix.
- `cd web && npm test -- src/api/client.test.ts src/router.test.ts`
- `nix develop -c bash -lc 'cd web && npm run test:e2e -- auth-flows.spec.ts'`
- `cd web && npm test && npm run build`; remove generated schema/dist, Playwright report, and test-results afterward.
- `cargo test --workspace`
- `cargo fmt -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Static inventory: no swallowed logout catch/redirect before success; explicit GET→POST helper; failure/Retry text; hard-navigation and failure/retry E2E markers; no token/cookie/session logging; exact staged allowlist; no browser/generated/process artifacts.
- No standalone VM test is required for this browser/backend session contract. Stop if implementation changes backend auth/session/CSRF/cookies or deployment configuration.
- `nix flake check`; preserve exact output if the known clean-baseline LLVM 21 `llvm-profdata merge` SIGILL recurs after tests pass.

## Dependencies

- Completed F-002 defines public bootstrap/login routing. Completed F-016 defines OpenAPI session+CSRF semantics. Both remain unchanged.
- Existing tolerant whoami GET returns 200 with optional username and an admin-response CSRF token; existing logout POST returns 204 after `AuthSession::logout` and cookie clearing. These are the behavioral authority.
- Existing API middleware captures `X-CSRF-Token` on every response and attaches it only to state-changing requests.
- No unresolved implementation dependency. Cleanup discovery begins automatically under D-111 after materialization; no cleanup unit is pre-approved.
