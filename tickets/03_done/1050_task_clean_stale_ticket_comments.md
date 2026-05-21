# Task: Clean stale ticket-number comments and "until then" references

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-009 (review report `backend-backendd_2026-05-21.md`)

## Background

Several comments in the crate refer to tickets that are in
`03_done/` and to transitional states that no longer exist. Two
levels of cleanup:

**Outright wrong** (delete or rewrite):

- `src/auth.rs:16-19` — "Ticket 212 builds on this for CSRF + logout;
  ticket 213 adds role middleware; ticket 214 adds bootstrap. **The
  bearer-token path in `server::authenticate_admin` continues to
  work in parallel during the transition.**" There is no
  bearer-token path; auth is session-cookie only.
- `src/admin_api/auth.rs:248-249` — "These two helpers will move to
  `admin_api::users` in ticket 770; until then they live in
  `crate::server` and the auth module reaches back for them." Both
  helpers (`mutate_collection`, `write_admin_user_audit`) already
  live in `admin_api::users`; the `use` line three lines above
  proves it.
- `src/web.rs:205` — "Build `web/` (ticket 220) or set the env var
  to a `dist/` directory.": ticket 220 is done.

**Footnote-style** `(ticket N)` markers on field rustdocs that
should describe the *why* instead of the ticket they came from:

- `src/config.rs:27` — `allowed_origins` — replace
  "Foundation for the CSRF work in ticket 212." with a sentence on
  what the field gates (state-changing requests must carry a
  matching Origin or Referer).
- `src/runtime_state.rs:66-68` — `session_revocations` — replace
  "(ticket 212)" with the operational reason (logout adds the id
  here so a stolen-but-unexpired cookie still rejects on the next
  request).
- `src/runtime_state.rs:71-75` — `bootstrap_lock` — replace
  "(ticket 214)" with "Serialises bootstrap attempts so two
  concurrent callers can't both pass the `is collection empty`
  check before either has written." (already partly there; just
  drop the ticket marker.)
- `src/runtime_state.rs:77-80` — `user_collection_lock` — replace
  "(ticket 240)" similarly.
- `src/auth.rs:72-75` — `session_id` doc — drop "(ticket 212)" but
  keep the existing explanation of how it differs from the AEAD
  nonce.

## Scope

Walk each location above and edit only the comment text. Production
behaviour is untouched.

## Out of scope

- Adding new comments where none existed.
- Rewriting working rustdocs that aren't called out above. This is
  a targeted sweep, not a doc pass.
- Touching `docs/*.md`. Different scope.

## Acceptance criteria

- [ ] `grep -rn "ticket [0-9]" crates/backend-backendd/src/`
      returns no matches (or only ones that explain a current
      behaviour and reference a still-meaningful ticket id, which
      is unlikely given the list above).
- [ ] `grep -n "bearer-token path\|until then" crates/backend-backendd/src/`
      returns no matches.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes (no behavioural change, but
      the rustdoc-doctest pass should still be green).
