# Task: Derive secretsd collection object paths from the collection name (fix Go keyring clients)

## Problem

Secret Service clients backed by the Go `99designs/keyring` library
(via `gsterjov/go-libsecret`) cannot persist secrets through our
secretsd. The write *appears* to succeed, but every subsequent run
reports the secret as missing. Affected tools include `gog` (gogcli),
`aws-sso`, and any other Go CLI using that library (aws-vault, chamber,
…). `libsecret`-C clients (`gh`, `secret-tool`, GNOME apps) are
unaffected, which is why this stayed hidden.

Symptom seen with gog under the wrapper:

```
gog auth add … → "Secret not found in keyring (refresh token missing)."
```

(That gogcli message is itself a mislabeled keyring-miss — not our bug.)

## Root cause

Our secretsd assigns each collection an opaque object-path segment from
a counter instead of from the collection name:

- `crates/underwrap-secretsd-store/src/store/collections.rs` —
  `create_collection()` calls `next_collection_id()` → `"col_1"`,
  `"col_2"`, … ignoring the requested Label.

But `99designs/keyring` locates *its* collection by **exact object
path**, not by the `label` property
(`secretservice.go`, `openSecrets`):

```go
collections, _ := service.Collections()
path := "/org/freedesktop/secrets/collection/" + k.name   // k.name = LibSecretCollectionName, defaults to ServiceName
for _, c := range collections {
    if c.Path() == path { k.collection = &c; return }      // match by PATH
}
// no match → ErrKeyNotFound
```

`k.name` is `gogcli` for gog and `awsssocli` for aws-sso. And
`go-libsecret`'s `CreateCollection` sends an **empty alias**
(`service.go`: `...CreateCollection", 0, properties, "")`), so no alias
is created either.

Concrete trace (verified via `debug-gog-castlabs` + the secretsd state
file `/tmp/.underwrap-ns-bus/libsecret-state.jcs`):

1. gog `Set`: `Collections()` has only `/collection/default`; gog wants
   `/collection/gogcli`; no match → it calls `CreateCollection(Label="gogcli", alias="")`.
2. secretsd mints id `col_1` → stores the item at
   `/collection/col_1` with `label="gogcli"`, **no alias**. Persisted state:
   ```json
   "aliases":{"default":"default"},
   "collections":{
     "col_1":{"label":"gogcli","items":{"item_2":{…client-secret…}}},
     "default":{"label":"Default","items":{}}
   }
   ```
   gog uses the returned `/collection/col_1` for the write → succeeds.
3. Next run gog `Get`: `Collections()` = `[/collection/col_1,
   /collection/default]`; gog wants `/collection/gogcli`; **neither path
   matches** → `errCollectionNotFound` → `ErrKeyNotFound`.

The secret is on disk the whole time, at a path gog never looks at. It
also means repeated writes keep minting fresh `col_N` collections
(non-idempotent litter).

`libsecret`-C works only because it uses the seeded **default**
collection (path `/collection/default`, which the secretsd creates) and
finds items by attribute search.

## Fix

Make the collection's object-path segment (its id) the **validated
collection name**, not a counter.

1. In `create_collection` (store layer
   `…-store/src/store/collections.rs`, and the D-Bus handler
   `crates/underwrap-secretsd/src/dbus_service/service.rs`):
   - Use the requested Label as the collection id: `id = name`.
   - **Idempotent by id:** if a collection with that id already exists,
     return it (do not mint a new one). This turns the client's
     create-if-missing into a clean no-op on every run after the first
     and removes the `col_N` litter.
   - Keep `next_collection_id()` (`col_N`) only as a fallback for the
     cases the validation explicitly allows through with no usable name
     — see Decisions.

2. Path/registration sites that must follow the new id:
   - `collection_path()` / `item_path()` (in `dbus_service.rs` / its
     submodules) already build `/collection/<id>` — they just receive
     the name-derived id now.
   - `crates/underwrap-secretsd/src/main.rs` registration loop
     (~L200-254) registers `/collection/<col_id>` from
     `list_collections()` — paths become `/collection/gogcli`
     automatically once the id is the name.
   - `section_store.rs` serialization is keyed by col_id; unchanged in
     shape, the keys are just names now.

3. Leave the seeded **`default`** collection and `default` alias exactly
   as-is (`section_store.rs` `store_from_section`). libsecret-C clients
   depend on it.

## Validation (reject, do not sanitize)

A D-Bus object-path element may contain only `[A-Za-z0-9_]` and must be
non-empty (note: a *leading digit* is legal for object paths). We
**require** names to already be valid and **fail** otherwise — no
character mapping/encoding. The name is untrusted input from the wrapped
target, so this is also a safety boundary (an unvalidated name flows
into an object path: `""` → invalid path, `foo/bar` → path-element
injection / collision with `/collection/<col>/<item>` item paths,
over-long → exceeds the 255-byte D-Bus path cap).

Add a validator, called in the `CreateCollection` D-Bus handler before
touching the store:

```rust
fn validate_collection_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() { return Err("collection name must not be empty"); }
    if name.len() > MAX_SEGMENT_LEN { return Err("collection name too long"); }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("collection name must match [A-Za-z0-9_]");
    }
    Ok(())
}
```

On failure return a proper D-Bus error (`fdo::Error::InvalidArgs`) so the
client sees it — never panic/`unwrap`, never silently fall back to a
`col_N` for an *illegal* name.

## Decisions (sensible defaults — override if you disagree)

- **Reject, don't sanitize** illegal names (per the above). All current
  real clients use already-valid names (`gogcli`, `awsssocli`, `login`,
  `default`), so this never rejects a real client.
- **Empty/missing Label → `InvalidArgs`** (do not invent a `col_N`).
- **`col_N` fallback** retained only if we ever choose to allow an
  anonymous collection through; with the reject-empty rule above it is
  effectively dead for the Secret Service path but harmless to keep.
- **No migration.** Pre-existing `col_N` collections from before the fix
  become orphaned but harmless (clients look at `/collection/<name>` and
  won't find them). The only data in `col_N` collections is
  re-acquirable credentials (OAuth/SSO tokens, OAuth client secrets) for
  gog/aws-sso — affected tools simply re-auth once. libsecret-C secrets
  live in `default` and are untouched. Operators can clear stale
  per-resource libsecret state if they want the litter gone.

## Out of scope

- The "embed the bus / drop dbus-daemon" idea — tracked separately; this
  fix is in the Secret Service service/store layer and is independent of
  the transport.
- The gogcli error-message wording (third-party).

## Acceptance

- New unit/integration tests:
  - `CreateCollection(Label="gogcli", alias="")` yields object path
    `/org/freedesktop/secrets/collection/gogcli`; same for `awsssocli`.
  - A second `CreateCollection` with the same name returns the **same**
    collection (idempotent), not a new one.
  - An item written to that collection is found by an exact-path lookup
    after a checkout → mutate → state-dump → re-checkout round-trip
    (mirrors the ns checkin/checkout cycle).
  - `CreateCollection` with `""`, `"a/b"`, and a non-ASCII name each
    return `InvalidArgs` and create nothing.
  - The seeded `default` collection/alias and a `secret-tool`/libsecret
    round-trip still work (no regression).
- End-to-end: `gog-castlabs auth add … --manual` persists, and a fresh
  invocation reads the client secret + token back (no "Secret not
  found"). Spot-check `aws-sso` persists through secret-service too.
- `cargo fmt -- --check`, `cargo build`, `cargo test`.

## Resolution

Implemented as specified.

- `underwrap-secretsd-store/src/store/collections.rs` — `create_collection`
  derives the id from the label (`id = name`) and is idempotent by id;
  `next_collection_id()` kept as the anonymous-label fallback.
- `underwrap-secretsd/src/dbus_service/service.rs` — added
  `validate_collection_name` (`[A-Za-z0-9_]`, non-empty, ≤128) called in the
  `CreateCollection` handler before the store mutation; rejects with
  `InvalidArgs`. No path-builder / `main.rs` / `section_store` changes were
  needed — they key off the id, which is now the name.

Tests: store-layer (id-is-name, idempotent, anonymous fallback); service
(name→path derivation, idempotent same-path, valid-name acceptance,
invalid-name rejection that persists nothing); section round-trip
(checkout → mutate → state-dump → re-checkout finds the secret at the
name-derived collection). The full D-Bus handler return value is not
unit-tested because `CreateCollection` requires a live `ObjectServer`
(a real bus); the validator + store seam is tested directly and the
end-to-end path is left to the VM/manual check in Acceptance.

Docs: `docs/secretsd-threat-notes.md` — corrected the now-false
"collection id is server-assigned" claims and documented the
name-as-path-segment decision, the non-conformant-but-gnome-keyring-matching
rationale, and the validation boundary. `docs/gh-libsecret.md` — listed the
now-supported Go `99designs/keyring` tools.

`cargo fmt --check`, `cargo clippy --all-targets --workspace -D warnings`,
and `cargo test --workspace` all pass. The end-to-end `gog-castlabs` /
`aws-sso` spot-checks in Acceptance are left for manual/VM verification.
