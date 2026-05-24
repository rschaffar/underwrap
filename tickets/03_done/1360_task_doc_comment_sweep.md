# Task: Backend WHAT-comment + stale-rustdoc sweep; tail cleanups

Resolves: B-DOC-1, B-DOC-2 (remainder), B-RT-8 (if not done in 1100),
B-COR-16, B-COR-17, B-ADM-26.

## Background

Final sweep across files touched by the earlier 1100-1300 tickets.
Two categories:

1. WHAT-comments that narrate the next line of code (delete on sight
   per `CLAUDE.md` "Tone and style").
2. Stale rustdoc paragraphs referring to code shapes that no longer
   exist (archaeology).

Plus a handful of tail cleanups that don't fit the bigger tickets.

## What to do

### WHAT-comments (B-DOC-1)

Delete or condense:

- `crates/backend-backendd/src/admin_api/mod.rs:90-110` — section
  banners ("tower-sessions + axum-login", "axum_csrf",
  "OpenApiRouter composition") narrating obvious code.
- `crates/backend-backendd/src/admin_api/csrf.rs:138-140` —
  explaining what `IntoResponse` on a tuple does.
- `crates/backend-backendd/src/admin_api/secrets.rs:332-334` —
  "Resolve material: caller-provided overrides; else carry the
  existing value across".
- `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:170-171` —
  "Run the record through `UserCollection::add` to re-use its
  uniqueness + hash invariant checks before we touch disk".
- `crates/backend-backendd/src/admin_api/libsecret.rs:58,144,197,330,499`
  — `---- shared helpers ----` and similar section banners.

Keep WHY-comments (atomicity invariants, security-critical decisions,
non-obvious constraints).

### Stale rustdoc (B-DOC-2 remainder)

- `crates/backend-backendd/src/admin_api/audit.rs:10-12` —
  "Per-subdomain wrappers used to format the entity string by hand;
  they have all been replaced by the `AuditEntity` enum…" —
  archaeology referring to a state that no longer exists.
- `crates/backend-backendd/src/admin_api/mod.rs:166-168` —
  `admin_openapi_spec` docstring describes the dual-construction
  antipattern. If `1110_task_admin_api_router_dedup` already
  landed this should be fixed; verify and update if not.
- `web/src/api/client.ts:516` — comment "of these go through the
  legacy flat-KV bridge" referring to a deleted code path
  (deleted in 1040, but double-check here).

### Tail cleanups

1. **Runtime double-logging** (B-RT-8):
   `crates/backend-backendd/src/runtime_api/mod.rs:65-68` —
   `warn!(err = format!("{e:#}"), "rejecting … {e:#}")` carries the
   same string twice. Keep `err` field, drop the interpolation.
   (Already covered by 1100; verify and finish if not.)

2. **`spa_unavailable()` HTML** (B-COR-16):
   `crates/backend-backendd/src/web.rs:92-107` returns an HTML 503
   with developer instructions when `web/dist/` is missing. The Nix
   flake closure guarantees the bundle for non-dev environments;
   release-quality binaries should never hit this path. Replace with
   a compile-time check — `build.rs` assertion or `compile_error!`
   macro guarded on a `cargo:rustc-env` reading
   `OPTION_ENV!("UNDERWRAP_WEB_DIST")`. A `cargo build` without
   `npm run build` then fails fast at build time instead of producing
   a binary that 503s on `GET /`.

3. **`tls.rs` `supported_algs` redundancy** (B-COR-17):
   `crates/backend-backendd/src/tls.rs:36-58` — drop the
   `supported_algs` instance field; reach for
   `default_provider().signature_verification_algorithms` directly
   in the `verify_tls12_signature` / `verify_tls13_signature`
   methods. Document that `install_default` and `default_provider`
   resolve to the same instance.

4. **Reconsider per-request `info!` log** (B-ADM-26):
   `crates/backend-backendd/src/admin_api/csrf.rs:169-174` logs
   `info!("admin request", method, path, user)` per request. The
   `path` includes operator-chosen ids and alias names that may be
   customer-recognizable. Per-handler `log_admin_event` already
   carries structured detail with redaction discipline. Choose:
   - Drop the per-request `info!` entirely (recommended).
   - Or log `method` + first-N path segments only.

## Simplification pass

After the cleanups above, walk every file touched by 1100-1300 once
more with the simplification lens from the review's category 7a:

- needless abstractions / wrapper types with a single caller
- defensive branches that can't fire (trust internal code)
- redundant `.clone()` / `to_owned()` calls
- single-purpose helpers that exist only for a hypothetical future
- options/flags never set to anything but the default
- error paths for impossible states
- WHAT-comments missed in the targeted sweep above

Remove anything that still fits.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo fmt -- --check`
- `cd web && npm test && npm run build`
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo build -p backend-backendd` without `web/dist/` populated
  fails at compile time (B-COR-16 acceptance).
