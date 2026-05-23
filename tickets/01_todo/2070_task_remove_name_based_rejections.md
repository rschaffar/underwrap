# Task: Remove operator-name rejections; names are labels, ids are generated

## Background

The SPA and one backend handler still reject operator-supplied
**names** on character-set or reserved-prefix grounds. But every
on-disk and on-URL identifier in this system is server-generated:

- Resource ids come from `generate_resource_id()`
  (`crates/backend-backendd/src/runtime_api/wrappers.rs`, used by
  `admin_api/resources.rs:109`).
- Store ids come from `crate::ids::fresh_id("store", …)`
  (`admin_api/stores/crud.rs:102`).
- User ids come from `crate::users::new_user_id()`.
- Client-instance ids come from `crate::ids::fresh_id("inst", …)`
  (`admin_api/client_instances.rs:192`).

Operator-supplied "names" are pure display labels. They do not
choose filenames, URL path segments, or anything else that needs
character-set gating. The remaining rejections defend against a
world that no longer exists.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-002.

## Scope

### SPA — drop the resource-name refinements

`web/src/pages/secrets/dialogs/create-resource.tsx:29-43`:

Replace the multi-refine zod schema with the minimal "must not be
empty" form:

```ts
const resourceSchema = z.object({
  resource: z.string().trim().min(1, "Resource name is required."),
});
```

Drop the three `.refine((s) => ...)` blocks (the `__underwrap/`
reservation, the leading/trailing-slash check, the
`env/files/libsecret` slot-segment check).

`web/src/pages/secrets/dialogs/rename-resource.tsx:29-37`:

Same treatment:

```ts
const renameResourceSchema = z.object({
  new_name: z.string().trim().min(1, "New name is required."),
});
```

Drop the `__underwrap` reservation `.refine`.

### SPA — drop the username regex

`web/src/pages/users/dialogs/create.tsx:35-41`:

Replace:

```ts
username: z
  .string()
  .min(1, "Username is required.")
  .regex(/^[a-z0-9][a-z0-9_-]*$/, "Use lowercase letters, digits, '-' or '_'; must start with a letter or digit."),
```

with:

```ts
username: z.string().trim().min(1, "Username is required."),
```

`web/src/pages/users/dialogs/edit.tsx:32-39`: same change. The
backend `normalise_username` (lowercase + trim) and its empty
check still apply on the wire.

### Backend — drop the dead `__underwrap` resource-id rejection

`crates/backend-backendd/src/storage/secret_entries.rs:143-148`:

Delete the `if resource_id.starts_with("__underwrap")` block. The
function receives a server-generated id; the check cannot fire.

Update the doc-comment on
`validate_and_compose_secret_id` (`:129-131`) to reflect that the
only operator-supplied input it validates is the per-slot key
shape (which still runs).

### What stays

The following rejections defend real runtime artifacts and must
**not** be touched:

- `crates/shared-protocol/src/resource.rs:189-207` —
  `validate_env_name` and `is_reserved_env_name` (the env-var name
  becomes a real process environment variable; `LD_*`, etc. is a
  security boundary).
- `crates/shared-protocol/src/resource.rs:96-111` —
  `validate_secret_key` for `Files` / `Libsecret` (the key becomes
  a path component inside the bind-mounted directory; path
  traversal still matters).
- The non-empty-after-trim checks on store name, user name,
  display name, resource name (those guard against zero-length
  rows, not against character content).

## Out of scope

- Renaming any field. "Name" stays "name".
- The duplicate-name uniqueness checks (those are about
  collisions, not character sets — keep them).
- Any change to id generation.

## Acceptance criteria

- [ ] `grep -n "__underwrap" web/src/pages/secrets/dialogs/` returns
      zero hits.
- [ ] `grep -n "^[a-z0-9]" web/src/pages/users/dialogs/` returns
      zero hits for the regex (the literal `/^[a-z0-9]` pattern is
      gone).
- [ ] `grep -n "starts_with(\"__underwrap\")"
      crates/backend-backendd/src/storage/secret_entries.rs`
      returns zero hits.
- [ ] `grep -rn "is_reserved_env_name\|validate_env_name\|validate_secret_key" crates/`
      still finds the kept-rules call sites unchanged (sanity
      check: don't accidentally rip out the env-name / path-traversal gates).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes.

## Notes for the implementer

- This is the explicit cutover the user asked for: names are
  labels. If you find another place in the codebase rejecting
  operator-supplied names on character-set or reserved-prefix
  grounds while leaving on-disk artifacts to a generated id,
  fold it into this same ticket — the principle is repo-wide.
- Re-check that no test asserts the dropped rejections still
  fire; the tests that move to the "name is now accepted" side
  should be updated in the same commit.
