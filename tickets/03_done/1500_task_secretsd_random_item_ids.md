# Task: Give Secret Service items random hex ids (fix cross-reload id collision / overwrite)

Related: `1490_task_secretsd_collection_path_from_name` (same root disease — a
counter that isn't reconciled with persisted state — but for items, and with a
worse symptom: silent data loss, not just not-found).

## Problem

Item ids are minted from a counter that resets on every secretsd startup and is
never reconciled with the loaded state, so a newly-created item can be given an
id that already exists and **silently overwrites** the existing item.

Real-world impact (confirmed): under the gog wrapper, `auth add` **overwrote the
stored OAuth `client-secret`** with its first token write. `auth add` itself
succeeded (it read the client secret before clobbering it), but the next command
(`drive search`) needs the client secret to refresh the access token, can't find
it, and fails with the mislabeled `keyring.ErrKeyNotFound` →
*"Secret not found in keyring (refresh token missing)."*

Verified via the secretsd state file
(`/tmp/.underwrap-ns-bus/libsecret-state.jcs`): after a real `auth add` the
`gogcli` collection holds three **token** items at `item_1`/`item_2`/`item_3`
and **no `client-secret` item** — it used to be `item_1` and was overwritten by
the first token write.

## Root cause

- `crates/underwrap-secretsd-store/src/store/model.rs` — a single shared
  `next_id: u64` counter (used by both collections and items).
- `crates/underwrap-secretsd-store/src/store/items.rs` — `next_item_id()` →
  `format!("item_{}", self.next_id)`; `create_item()` (replace=false branch)
  does `col.items.insert(id, item)` — a **blind insert that overwrites** any
  existing item with that id.
- `crates/underwrap-secretsd-store/src/store/collections.rs` — `new_default()`
  sets `next_id = 1`.
- `crates/underwrap-secretsd/src/section_store.rs` — `store_from_section()`
  rebuilds collections/items from the checked-out state but **never seeds
  `next_id`**, so it is `1` on every fresh secretsd (i.e. every wrapper call).

Trace: invocation A (`auth credentials set`) stores client-secret as `item_1`.
Invocation B (`auth add`) loads that state, `next_id` resets to `1`, reads the
client secret, then writes tokens: the first token (new attributes, no
replace-match) mints `next_item_id()` = `item_1` again →
`insert("item_1", token)` overwrites the client secret.

## Fix

Item ids carry no meaning and are only ever looked up by **attribute search**
(`Collection.SearchItems`), never by path or sort order, so make them
collision-proof random ids instead of a counter:

- **16 random bytes, lowercase hex-encoded** (32 chars). Hyphenless on purpose —
  the id becomes a D-Bus object-path segment (`/collection/<col>/<item>`), and a
  path segment may only contain `[A-Za-z0-9_]`; a canonical (hyphenated) UUID
  would be an illegal segment (the same bug class as 1490). 32 hex chars are all
  valid. (Optional `item_` prefix for readability — harmless; hex alone is also
  a valid segment.)
- Use `getrandom` (already in the dependency tree via `rand`; add it as a direct
  dep of `underwrap-secretsd-store`) to fill `[u8; 16]`, then hex-encode (manual
  `{:02x}` fold — no new hex dep needed; `base64` is already present but hex is
  preferred for path-segment legibility). Match whatever `getrandom` API version
  is already in the tree.
- Keep the **replace-by-attributes** path in `create_item` exactly as is (an
  item with matching attributes is still updated in place, not duplicated).

### Remove the now-dead counter

With 1490, collection ids are derived from the validated name and the D-Bus
`CreateCollection` handler rejects empty/illegal names, so the `col_N` fallback
(`next_collection_id`) is unreachable. Once items use random ids, the shared
`next_id` field has no remaining users:

- Delete `next_id` from `SecretStore` (`model.rs`) and from `new_default()`.
- Delete `next_item_id()` and `next_collection_id()`.
- If we keep any collection-name fallback at all, give it a random id too (not a
  counter). Preferred: drop the fallback and rely on the handler's
  validate-and-reject.

This removes the entire "counter vs persisted state" class of bug at the root —
no counter, no seeding, no shared mutable id state to get out of sync.

## Constraints / notes

- Ids must stay valid D-Bus path segments (`[A-Za-z0-9_]`, non-empty).
- No client depends on item id values (attribute search only), so opaque random
  ids are safe.
- Verify `getrandom` is not blocked by the secretsd's namespace/seccomp posture
  (almost certainly fine — it's a basic syscall and the secretsd handles secrets
  already), and fail loudly if randomness is unavailable rather than falling back
  to a predictable id.

## Data repair (existing stores)

Stores written before this fix may have a **clobbered `client-secret`** (and the
exposed refresh token should be revoked at Google — handled separately). After
this lands and is deployed:

1. Re-store the client secret: `gog-<acct> auth credentials set <client.json>`.
   With random ids it gets a fresh id and the existing token items are untouched.
2. Re-authorize the account (the prior refresh token was revoked).

⚠️ Do **not** re-run `auth credentials set` on an unfixed build — it will mint a
colliding `item_1` and clobber a token item.

## Acceptance

- New item ids match `^[0-9a-f]{32}$` (or `^item_[0-9a-f]{32}$` if prefixed).
- Regression: build a store with an existing item X, run it through
  `store_from_section` (reload), then `create_item` a *different* item Y →
  **X is still present** and Y has a distinct id (no overwrite). This is the
  exact scenario that destroyed the client secret.
- `create_item(replace=true)` with matching attributes still updates in place
  (one item, not two).
- A checkout → mutate → state-dump → re-checkout round-trip preserves all items.
- `next_id` and `next_item_id`/`next_collection_id` are gone (no remaining
  references).
- End-to-end: after re-storing credentials, `gog-castlabs auth add` followed by a
  fresh-invocation `drive search` both succeed (client secret survives the token
  writes).
- `cargo fmt -- --check`, `cargo build`, `cargo test`.

## Out of scope

- Collection naming/validation — done in 1490.

## Resolution

Implemented as specified, including the counter removal.

- `underwrap-secretsd-store/src/store/items.rs` — new `random_item_id()`:
  16 bytes from `getrandom::fill`, lowercase-hex (manual `{:02x}` fold, no
  hex dep), fails loudly via `Result` if the OS RNG is unavailable.
  `create_item` uses it; the `replace`-by-attributes path is unchanged.
- `getrandom = "0.3"` added to workspace deps + the store crate (0.3.4 was
  already in the lock — no new crate version). `getrandom::Error` isn't an
  `anyhow`-compatible `StdError`, so it's mapped via its `Display`.
- Counter fully removed: deleted `next_id` (`model.rs`), `next_item_id()`,
  and `next_collection_id()`. `new_default()` no longer seeds `next_id`.
  `create_collection` dropped the now-dead anonymous fallback — its
  signature is now `(name: &str, alias)`, id = the validated name (the
  D-Bus handler already rejects empty/illegal names), so no RNG and no
  fallback id are needed on the collection path.

Tests: `item_ids_are_random_hex` (`^[0-9a-f]{32}$`, distinct ids);
`reload_then_create_item_does_not_overwrite_existing` (the exact
store_from_section reload → create-different-item scenario that destroyed
the client secret — X survives, Y distinct); existing
`replace_item_by_attributes` covers in-place replace; the checkout
round-trip test from 1490 still passes. All `create_collection` call sites
updated to the `&str` signature; the 1490 anonymous-fallback test removed.

Docs: `docs/secretsd-threat-notes.md` — item-id note now describes the
random server-generated ids and the data-loss bug they fix; dropped the
stale `col_N`/counter references.

`cargo fmt --check`, `cargo clippy --all-targets --workspace -D warnings`,
and `cargo test --workspace` all pass (one integration test,
`pair_last_used_is_bumped_on_successful_wrapper_read`, is a pre-existing
`TestDaemon::start` concurrency flake under the full parallel run — passes
in isolation and on re-run, unrelated to item-id generation). The
end-to-end `gog-castlabs auth add` → `drive search` check and the existing-
store data repair in "Data repair" are left for manual/deploy-time
verification.
