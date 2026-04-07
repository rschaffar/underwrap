# libsecret mode — implementation plan

## Goal

Wrappers can provide a private D-Bus session with a Secret Service API
provider to the target process. This enables tools like `gh` that use
libsecret to store and retrieve credentials via `org.freedesktop.secrets`
to work transparently — storing to the underwrap backend on write and
reading from it on read.

This allows full bootstrap flows like `gh auth login` where the tool
performs an OAuth flow and stores the resulting token via libsecret,
with the actual storage going to the underwrap backend (local or remote).

## Architecture

```
PID namespace (created by underwrap-ns)
┌─────────────────────────────────────────────────────┐
│                                                     │
│  dbus-daemon (private session bus)                  │
│    └── unix:path=/tmp/underwrap-dbus-<pid>/bus      │
│                                                     │
│  underwrap-secretsd                                 │
│    ├── owns org.freedesktop.secrets on private bus   │
│    ├── implements Secret Service D-Bus API           │
│    ├── reads/writes via backend socket or local file │
│    └── metadata (attributes, labels) in redb/JSON    │
│                                                     │
│  target (e.g. gh)                                   │
│    ├── DBUS_SESSION_BUS_ADDRESS=unix:path=...       │
│    ├── libsecret connects to private bus             │
│    ├── finds org.freedesktop.secrets → our provider  │
│    └── CreateItem / GetSecret work transparently     │
│                                                     │
└─────────────────────────────────────────────────────┘
```

## Key design decisions

### Private D-Bus bus, not the system session bus

The target runs in an isolated PID namespace. It must NOT connect to the
system D-Bus session bus (which would bypass isolation and expose the
system keyring). Instead, `underwrap-ns` launches a private `dbus-daemon`
on a local socket inside the namespace, and `underwrap-secretsd` connects
to that bus.

### dbus-daemon as the bus broker

libsecret clients expect a proper D-Bus bus with name resolution
(`org.freedesktop.DBus`), not a peer-to-peer connection. The simplest
correct approach is to run `dbus-daemon --session --address=unix:path=...`
inside the namespace. This is a standard, well-tested component.

### Metadata storage

The Secret Service API has concepts beyond raw secret values:
- **Collections**: groups of items (we map to underwrap instances)
- **Items**: individual secrets with labels and attributes
- **Attributes**: key-value pairs for searching (e.g. `service=gh, account=github.com`)
- **Labels**: human-readable names
- **Created/Modified timestamps**

Our `StorageBackend` trait only does `read(id) → String` and `write(id, material)`.
The metadata (attributes, labels, timestamps) needs a separate store. Options:
- **redb** (as `pass-secret-service` uses): embedded key-value database
- **JSON sidecar files**: one JSON per item alongside the secret
- **Single JSON file**: all metadata in one file

Recommendation: **redb** for robustness, or **single JSON file** for simplicity.
The JSON approach is easier to inspect and debug.

## New crates

### `underwrap-secretsd` (binary)

The D-Bus Secret Service provider. Connects to a D-Bus session bus and
implements `org.freedesktop.Secret.Service`, `Collection`, `Item`, `Session`.

Dependencies: `zbus` v4 (with tokio), `serde`, `serde_json`, `tokio`.

Structure (following `pass-secret-service` patterns):

```
crates/underwrap-secretsd/
  src/
    main.rs              # CLI entry point, bus connection
    dbus_server/
      mod.rs             # Module declarations
      service.rs         # org.freedesktop.Secret.Service interface
      collection.rs      # org.freedesktop.Secret.Collection interface
      item.rs            # org.freedesktop.Secret.Item interface
      session.rs         # org.freedesktop.Secret.Session (plain algorithm)
      secret.rs          # Secret type (session, params, value, content_type)
      utils.rs           # Path construction helpers
    store.rs             # Metadata store (attributes, labels → JSON file)
```

Approximate size: ~2000-2500 lines based on `pass-secret-service` reference
(which is ~3100 lines including DH encryption, process tracking, redb, and
CLI features we don't need).

What we can skip vs `pass-secret-service`:
- **DH key exchange**: we only need "plain" sessions (local Unix socket)
- **Process tracking**: nice but not essential
- **redb**: use JSON instead
- **GPG integration**: not needed (we have our own backend)
- **CLI subcommands**: not needed

What we must implement:
- `OpenSession` (plain only)
- `SearchItems` (attribute matching)
- `GetSecrets` (bulk read)
- `CreateCollection` (with default collection)
- `ReadAlias` / `SetAlias` (for "default" alias)
- `Collection.CreateItem` (with replace semantics)
- `Collection.SearchItems`
- `Collection.Delete`
- `Item.GetSecret` / `Item.SetSecret`
- `Item.Delete`
- `Item.Attributes` (property, get/set)
- `Item.Label` (property, get/set)
- Properties: `Collections`, `Locked`, `Created`, `Modified`
- Signals: `CollectionCreated`, `CollectionDeleted`, `ItemCreated`, `ItemDeleted`

### `underwrap-dbus-store` (library crate, internal)

Reusable metadata storage for Secret Service items. Decoupled from D-Bus
so it can be tested independently.

```rust
pub struct SecretMetadataStore { ... }

impl SecretMetadataStore {
    pub fn load_or_create(path: &Path) -> Result<Self>;
    pub fn save(&self) -> Result<()>;

    // Collections
    pub fn create_collection(&mut self, label: Option<String>, alias: Option<String>) -> String;
    pub fn delete_collection(&mut self, id: &str);
    pub fn list_collections(&self) -> Vec<String>;
    pub fn get_alias(&self, alias: &str) -> Option<String>;
    pub fn set_alias(&mut self, alias: &str, collection_id: Option<String>);

    // Items
    pub fn create_item(
        &mut self, collection: &str, label: &str,
        attributes: &HashMap<String, String>, secret: &[u8],
        replace: bool,
    ) -> String;
    pub fn delete_item(&mut self, collection: &str, id: &str);
    pub fn search(&self, collection: &str, attributes: &HashMap<String, String>) -> Vec<String>;
    pub fn search_all(&self, attributes: &HashMap<String, String>) -> Vec<(String, Vec<String>)>;
    pub fn get_secret(&self, collection: &str, id: &str) -> Option<Vec<u8>>;
    pub fn set_secret(&mut self, collection: &str, id: &str, value: &[u8]);
    pub fn get_attributes(&self, collection: &str, id: &str) -> Option<HashMap<String, String>>;
    pub fn set_attributes(&mut self, collection: &str, id: &str, attrs: HashMap<String, String>);
    pub fn get_label(&self, collection: &str, id: &str) -> Option<String>;
    pub fn set_label(&mut self, collection: &str, id: &str, label: &str);
}
```

Backed by a JSON file. Thread-safe via `Arc<Mutex<...>>` wrapping.

This crate holds no D-Bus code. Pure data operations. Fully unit-testable.

## Changes to existing crates

### `underwrap-ns`

When libsecret mode is enabled (via `UNDERWRAP_SECRETSD` env var):

1. Create a temp directory for the private bus socket
2. Launch `dbus-daemon --session --address=unix:path=<socket> --nofork &`
3. Wait for the socket to appear
4. Launch `underwrap-secretsd <store-path> --bus-address unix:path=<socket> &`
5. Wait for secretsd to acquire `org.freedesktop.secrets`
6. Set `DBUS_SESSION_BUS_ADDRESS=unix:path=<socket>` on the target
7. Exec the target
8. On exit: the namespace dies, all child processes (dbus-daemon, secretsd) are killed

### `underwrap-exec`

Passes `UNDERWRAP_SECRETSD` and `UNDERWRAP_SECRETSD_STORE` env vars to the
child that execs `underwrap-ns`, based on the sidecar config `libsecret: true`.

### NixOS module

- New `libsecret` boolean option on wrapper definitions
- When enabled, sidecar config includes `secretsd_binary` and `secretsd_store` paths
- `dbus` package available in the namespace (for `dbus-daemon`)
- `underwrap-secretsd` binary installed alongside other binaries

## Store file location

Per-wrapper secretsd store: `<state-dir>/secretsd/<wrapper-name>.json`

This allows different wrappers to have separate Secret Service collections.
A `gh` wrapper has its own store; a `slack` wrapper has its own.

## Bootstrap flow example

```bash
# 1. User runs the wrapper with --bootstrap or just normally
$ work-gh-authorized auth login

# 2. gh does OAuth flow in the browser, gets a token
# 3. gh calls libsecret CreateItem to store the token
# 4. Our secretsd intercepts, stores in <state-dir>/secretsd/gh-authorized.json
# 5. Done — token is now in the underwrap backend

# Next time:
$ work-gh-authorized pr list
# gh calls libsecret SearchItems + GetSecret
# Our secretsd serves from the JSON store
# gh gets the token, works normally
```

With remote backend: the secretsd store file would be on the remote backend
(not local disk), so the token is stored on your workstation, not the VM.
This requires the secretsd to use the backend protocol instead of a local
file — a future enhancement.

## Testing

### Unit tests (underwrap-dbus-store)

- Create/delete collections
- Create/delete items
- Attribute search (exact match, subset match)
- Replace semantics on CreateItem
- Alias resolution
- JSON persistence (save/load roundtrip)

### Integration test

- Start `dbus-daemon` + `underwrap-secretsd` on a temp socket
- Use `secret-tool` (from libsecret) or the `secret-service` Rust crate as a client
- Store a secret with attributes
- Search and retrieve it
- Verify it persists in the JSON file

### VM test

- Configure a wrapper with `libsecret = true`
- Run `secret-tool store --label "test" service gh account github.com` via the wrapper
- Run `secret-tool lookup service gh account github.com` via the wrapper
- Verify the secret roundtrips
- Verify the secretsd JSON file contains the data
- Verify no D-Bus traffic leaks to the system session bus

## Estimated effort

| Component | Lines | Complexity |
|-----------|-------|------------|
| `underwrap-dbus-store` (metadata lib) | ~400 | Low — JSON CRUD |
| `underwrap-secretsd` (D-Bus server) | ~2000 | High — D-Bus protocol |
| `underwrap-ns` changes | ~50 | Low — launch dbus-daemon + secretsd |
| `underwrap-exec` changes | ~20 | Low — pass config through |
| NixOS module changes | ~30 | Low — add option, package |
| Tests | ~300 | Medium |
| **Total** | **~2800** | |

## Reference implementation

`pass-secret-service` (github.com/grimsteel/pass-secret-service):
- Pure Rust, ~3100 lines
- Uses zbus v4 with tokio
- Full Secret Service implementation including DH encryption
- Clean separation: `dbus_server/` handles D-Bus, `secret_store/` handles persistence
- Well-structured `#[interface]` macros showing correct zbus v4 API usage
- Licensed under GPL-3.0 — our implementation must be independent, not a fork

## Dependencies

New workspace dependencies:
- `zbus` v4 (with tokio feature) — already attempted, needs careful API usage
- `dbus` system package in the PID namespace (NixOS provides this)

No new system library dependencies (zbus is pure Rust).
