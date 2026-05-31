# secretsd threat notes

`underwrap-secretsd` is the one frontend component that parses bytes
supplied by the **target** process — the most potentially-hostile process
in the system. It speaks the D-Bus Secret Service protocol
(`org.freedesktop.secrets`) on a private session bus inside the wrapper's
PID + mount namespace. The target may have been compromised; everything
secretsd accepts over D-Bus must be treated as adversarial.

This document records the assumptions each D-Bus handler makes about its
inputs, what happens if those assumptions are violated, and the mitigations
in place. It is the written output of the adversarial review in ticket
1480.

## Trust boundaries

- **Target → secretsd (D-Bus):** fully untrusted. Covered below.
- **`underwrap-ns` → secretsd (`--init-fd`, CLI args):** trusted. ns is part
  of the wrapper chain and runs before the target; it hands secretsd the
  initial keyring bytes and the state path. A compromise of ns is a
  compromise of the whole namespace, so secretsd does not defend against
  malicious ns input. `read_init_section` reads the pipe to EOF; ns closes
  the write end after writing the section, so the read terminates. The
  producer is trusted and the size is bounded by the resource blob the
  backend served.
- **secretsd → state file (`--state-path`):** secretsd is the sole writer.
  The file lives on a `size=4m` tmpfs inside the namespace.

## Mitigations in place

- **Persisted-state size cap** (`state.rs`): every D-Bus mutation
  re-serialises the whole keyring and persists it. Before touching the
  file, `dump` rejects any state whose canonical form exceeds
  `DEFAULT_MAX_STATE_BYTES` (1 MiB, overridable via `--max-state-bytes`).
  This bounds how large a compromised target can grow the keyring via
  repeated `CreateItem` / `SetSecret` calls — it cannot fill the tmpfs or
  drive unbounded IO churn. A rejected mutation surfaces as a `Failed`
  D-Bus error and leaves the previously-persisted (valid) state intact (the
  cap is checked before the on-disk file is opened, and the atomic
  write-to-`.new`-then-rename means a rejected write leaves no temp file).
- **Canonicalisation invariant** (tested in
  `section_store/tests.rs::target_modifiable_values_always_canonicalise_and_round_trip`):
  any value the target can set — item labels, attribute keys/values,
  content types, and arbitrary secret bytes (including embedded NULs and
  non-UTF-8) — projects into a `LibsecretSection` that canonicalises (JCS,
  RFC 8785) without error and round-trips stably. Secret payloads are
  carried as base64 in the canonical form, so non-UTF-8 bytes are safe.
- **Object-path construction** (`main.rs`, `service.rs`): collection / item
  / alias object paths are built with `ObjectPath::try_from`, which rejects
  illegal D-Bus paths; failures are logged and the registration is skipped
  rather than panicking. Item ids are **server-generated** (16 random bytes,
  lowercase-hex via `random_item_id`), never target-chosen, so a target
  cannot inject path separators via an id. Random ids also replace an
  earlier reset-on-startup counter that could re-mint an id already present
  in the reloaded state and silently overwrite that item (the gog
  client-secret data-loss bug); 128 bits makes a collision negligible, and
  the `create_item` `replace`-by-attributes path is unaffected.
- **Collection-name validation** (`service.rs` `validate_collection_name`):
  a collection's object-path segment **is** its target-chosen name (see
  "CreateCollection" below for why), so the name is the one piece of
  target input that flows directly into a path. `CreateCollection`
  validates it *before* touching the store and rejects with `InvalidArgs`
  anything that is empty, longer than 128 bytes, or outside `[A-Za-z0-9_]`
  — the D-Bus path-element charset. Reject, never sanitize: this closes
  path-element injection (`foo/bar` colliding with
  `/collection/<col>/<item>` item paths), empty/over-long paths, and the
  D-Bus 255-byte path cap. An invalid name persists nothing.

## Per-handler assumptions

### `org.freedesktop.Secret.Service`

- **OpenSession** — only the `plain` algorithm is accepted; any other
  algorithm returns `NotSupported`. The client-supplied `input` variant is
  ignored (no DH key exchange), so there is no client-controlled buffer to
  size-attack here.
- **CreateCollection** — label/alias are read from client properties. The
  Label is the collection **name**, and its validated form becomes the
  collection's object-path segment (its id) — *not* an opaque
  counter. This is deliberate: Secret Service clients backed by Go's
  `99designs/keyring` (gog, aws-sso, aws-vault, chamber, …) locate their
  collection by exact object path rather than by the `Label` property or an
  alias. That is technically non-conformant with the Secret Service spec
  (which treats paths as opaque), but it matches how gnome-keyring behaves
  in practice (paths derived from stable keyring names), so honouring the
  name is what lets those clients read back what they wrote. The name is
  validated first (see "Collection-name validation"); creation is
  idempotent by id, so a client's create-if-missing is a clean no-op after
  the first run instead of minting a fresh collection each run. Persists via the
  size-capped `dump`. The seeded `default` collection and alias are
  untouched; spec-conformant clients (libsecret-C: `gh`, `secret-tool`,
  GNOME apps) discover by `Label`/`ReadAlias`/attribute search and are
  unaffected by the path segment.
- **SearchItems** — read-only; matches attributes against the in-memory
  store and returns object paths. No mutation, no allocation proportional to
  attacker input beyond the result vector.
- **GetSecrets** — read-only; returns secrets for the given item paths.
  Unknown/ill-formed paths are skipped (`parse_item_path` returns `None`).
- **ReadAlias / SetAlias** — alias name is target-supplied; `SetAlias`
  persists via the capped `dump`. Alias strings are stored as map keys and
  canonicalise without issue (see invariant test).
- **Unlock / Lock** — no-ops in this model (no locking); return the inputs.
- **Collections** (property) — read-only listing.

### `org.freedesktop.Secret.Collection`

- **Delete** — removes the collection and persists. Idempotent on a missing
  collection.
- **SearchItems** — read-only, scoped to this collection.
- **CreateItem** — the primary growth vector. Label/attributes are parsed
  from client properties (defaulting on absence/ill-typing); the secret is
  arbitrary bytes. `replace` reuses an existing item when attributes match.
  Persisted through the size-capped `dump`, which is what bounds repeated
  creation.
- **Items / Label / SetLabel / Locked / Created / Modified** — small
  getters; `SetLabel` persists via the capped `dump`.

### `org.freedesktop.Secret.Item`

- **Delete** — removes the item and persists; idempotent on a missing item.
- **GetSecret** — read-only; returns the stored secret, or `Failed` if the
  item is gone.
- **SetSecret** — overwrites the secret/content-type of an existing item
  (silently no-ops if the item was concurrently deleted) and persists via
  the capped `dump`.
- **SetAttributes** — replaces the attribute map and persists.
- **Attributes / Label / SetLabel / Locked / Created / Modified** — getters
  and small setters; setters persist via the capped `dump`.

## Known follow-ups (not done in 1480)

- A dedicated `cargo-fuzz` harness over the D-Bus message path was **not**
  added: it needs a nightly toolchain and a separate fuzz crate, which is a
  toolchain/dependency decision to make explicitly. The generative
  canonicalisation test above covers the JCS write-path surface in the
  normal `cargo test` run; a fuzz harness would extend that and should be
  wired as a non-blocking CI smoke run when the toolchain question is
  settled.
