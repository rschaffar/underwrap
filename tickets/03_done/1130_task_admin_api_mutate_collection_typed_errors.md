# Task: `mutate_collection` — typed errors, drop the diff

Resolves: B-ADM-8, B-ADM-9, B-ADM-10.

## Problem

`crates/backend-backendd/src/admin_api/users.rs:48-74` defines
`mutate_collection` as:

1. Snapshot `before: Vec<UserRecord>`.
2. Clone to `after: Vec<UserRecord>`.
3. Run the closure on `after`.
4. Diff before/after with two `HashMap<&str, &UserRecord>` allocations,
   emit save/delete calls for the changed records.

Each handler mutates exactly one record (create-one, update-one,
reset-one, delete-one). The diff machinery is unjustified.

Worse, the closure returns `anyhow::Result<T>` while every handler
wants typed `UserMutationError`. The crate then `downcast_ref::<UserMutationError>()`
in `classify_mutation_error`; if downcast fails, an IO error from
`save_user` surfaces as a 400 with the IO message in the body
(`crates/backend-backendd/src/admin_api/users.rs:107-115`).

Also: `audit_user_mutation` (`:123-157`) takes a `FnOnce(&T) -> &str`
extractor in four shapes (`|r| &r.username`, `|u| &u.username`, `|s|
s.as_str()`, `|r| &r.username`) because the success types are
inconsistent.

## What to do

1. Change `mutate_collection`'s closure signature to
   `FnOnce(&mut UserCollection) -> Result<T, UserMutationError>`.
2. Define
   ```rust
   pub(crate) enum MutateCollectionError {
       User(UserMutationError),
       Io(anyhow::Error),
   }
   ```
   `mutate_collection` returns `Result<T, MutateCollectionError>`.
3. Either drop the diff entirely and have the closure return
   which records to persist, or move persistence into the handler
   (each one knows the id it touched). Pick whichever reads cleaner.
4. Handlers `match` cleanly: `User(_)` → 400 with code, `Io(_)` → 500.
   Delete `classify_mutation_error`'s downcast fallback.
5. Normalize `audit_user_mutation`: have handlers map the
   `mutate_collection` result to `(String /*username*/, payload)`;
   the audit helper then takes just `(action, principal, username,
   result)`. Removes the generic parameter and the four success-extractor
   closures.

## Acceptance

- `cargo test --workspace`
- Add a unit test: IO failure during `save_user` surfaces as 500
  (previously surfaced as 400).
- `cargo clippy --all-targets --workspace -- -D warnings`
