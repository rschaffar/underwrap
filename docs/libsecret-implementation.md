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
│    ├── reads/writes via daemon channel (fd 4)        │
│    └── all state in memory, persisted via channel     │
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

### Storage: structured envelope through the daemon

The Secret Service API has concepts beyond raw secret values:
- **Collections**: groups of items (we map to underwrap instances)
- **Items**: individual secrets with labels and attributes
- **Attributes**: key-value pairs for searching (e.g. `service=gh, account=github.com`)
- **Labels**: human-readable names
- **Created/Modified timestamps**

Our `StorageBackend` trait only does `read(id) → String` and `write(id, material)`.
The solution is a **structured JSON envelope**: the entire libsecret state for
a wrapper (collections, items, attributes, labels, AND secret values) is
serialized into a single JSON blob and stored as one secret in the backend
under the key `<wrapper-name>/libsecret`.

**ALL secret material flows through StorageBackend** — see `security.md`,
"Design invariants". No local JSON files, no sidecar databases, no redb.

This works because each wrapper's libsecret state is small (a handful of
items with short tokens). Search is just deserialize + in-memory attribute
matching. The backend provides encryption at rest, remote storage, and
audit logging for free.

### Secretsd channel: authenticated socketpair

Secretsd runs inside the PID namespace as the regular user — it can't
authenticate to the daemon's read socket (no setgid GID). Instead, the
daemon creates a `socketpair()` during the GID-authenticated wrapper
request and passes one end via `SCM_RIGHTS`. The other end stays with the
daemon as a scoped handler that only permits read/write of that wrapper's
libsecret secret.

The fd is inherited through the fork/exec chain to secretsd. It is
**closed before exec'ing the target** — only secretsd holds it.

See `security.md`, "Secretsd channel" for the full security analysis.

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
    store.rs             # Secret store (JSON envelope via daemon channel)
```

Approximate size: ~2000-2500 lines based on `pass-secret-service` reference
(which is ~3100 lines including DH encryption, process tracking, redb, and
CLI features we don't need).

What we can skip vs `pass-secret-service`:
- **DH key exchange**: we only need "plain" sessions (local Unix socket)
- **Process tracking**: nice but not essential
- **redb**: not needed (structured envelope through backend)
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

### `underwrap-secretsd-store` (library crate, internal)

In-memory Secret Service state backed by a JSON envelope that is read from
and written to the daemon via the secretsd channel (fd-based IPC). Decoupled
from D-Bus so it can be tested independently.

The entire state (collections, items with their attributes, labels,
timestamps, AND secret values) lives in a single serializable struct.
On startup, secretsd reads the envelope from the daemon channel. On
mutation, it writes the updated envelope back.

```rust
pub struct SecretStore { ... }

impl SecretStore {
    /// Deserialize from JSON envelope (read from daemon channel at startup).
    pub fn from_json(data: &str) -> Result<Self>;
    /// Empty store with a default collection.
    pub fn new_default() -> Self;
    /// Serialize to JSON envelope (written to daemon channel on mutation).
    pub fn to_json(&self) -> Result<String>;

    // Collections
    pub fn create_collection(&mut self, label: Option<&str>, alias: Option<&str>) -> String;
    pub fn delete_collection(&mut self, id: &str) -> bool;
    pub fn list_collections(&self) -> Vec<&str>;
    pub fn get_alias(&self, alias: &str) -> Option<&str>;
    pub fn set_alias(&mut self, alias: &str, collection_id: Option<&str>);

    // Items
    pub fn create_item(
        &mut self, collection: &str, label: &str,
        attributes: &HashMap<String, String>, secret: &[u8],
        content_type: &str, replace: bool,
    ) -> Result<(String, bool)>; // (item_id, replaced_existing)
    pub fn delete_item(&mut self, collection: &str, id: &str) -> bool;
    pub fn search(&self, collection: &str, attributes: &HashMap<String, String>) -> Vec<&str>;
    pub fn search_all(&self, attributes: &HashMap<String, String>) -> Vec<(&str, Vec<&str>)>;
    pub fn get_secret(&self, collection: &str, id: &str) -> Option<&[u8]>;
    pub fn set_secret(&mut self, collection: &str, id: &str, value: &[u8]) -> bool;
    pub fn get_attributes(&self, collection: &str, id: &str) -> Option<&HashMap<String, String>>;
    pub fn set_attributes(&mut self, collection: &str, id: &str, attrs: HashMap<String, String>) -> bool;
    pub fn get_label(&self, collection: &str, id: &str) -> Option<&str>;
    pub fn set_label(&mut self, collection: &str, id: &str, label: &str) -> bool;
    pub fn get_created(&self, collection: &str, id: &str) -> Option<u64>;
    pub fn get_modified(&self, collection: &str, id: &str) -> Option<u64>;
}
```

Thread-safe via `Arc<RwLock<...>>` wrapping in secretsd.

This crate holds no D-Bus code. Pure data operations. Fully unit-testable.

**Invariant**: the JSON envelope contains secret values. It is NEVER written
to a local file. It is only serialized/deserialized over the daemon channel
(authenticated socketpair fd) or in tests.

## Changes to existing crates

### `underwrapd` (daemon)

New capabilities for libsecret support:

1. **Secretsd channel creation**: when a libsecret-enabled wrapper connects,
   the daemon creates a `socketpair()` and sends one fd back via `SCM_RIGHTS`
   alongside the normal secret response.
2. **Secretsd channel handler**: a spawned task that services read/write
   requests over the socketpair, scoped to `<wrapper-name>/libsecret`.
   Uses a simple framed protocol (same as existing IPC framing).
3. **Dynamic secret support**: secrets with the `/libsecret` suffix are
   auto-declared as mutable when a wrapper has `libsecret: true`.

New message types:
```rust
// Secretsd channel requests (over the socketpair)
enum SecretsdRequest {
    Read,                      // read the libsecret envelope
    Write { envelope: String }, // write updated envelope
}
enum SecretsdResponse {
    Ok { envelope: Option<String> },
    Error { message: String },
}
```

### `underwrap-exec`

1. Sidecar config gains `libsecret: true` field.
2. When enabled, the daemon response includes a `SCM_RIGHTS` fd for the
   secretsd channel.
3. `underwrap-exec` passes both fd 3 (secret pipe) and fd 4 (secretsd
   channel) to `underwrap-ns`. The channel fd is passed alongside the
   sidecar config fields (`secretsd_binary`, `dbus_daemon_binary`).

### `underwrap-ns`

When libsecret mode is enabled (fd 4 is open, sidecar has secretsd config):

1. Create a temp directory for the private bus socket
2. Fork → child A: exec `dbus-daemon --session --address=unix:path=<socket> --nofork`
3. Wait for the bus socket to appear (poll with timeout)
4. Fork → child B: exec `underwrap-secretsd --bus-address unix:path=<socket> --channel-fd 4`
   (child B inherits fd 4)
5. **Close fd 4** — critical: target must not inherit it
6. Wait for secretsd readiness (secretsd writes a byte to a readiness pipe)
7. Set `DBUS_SESSION_BUS_ADDRESS=unix:path=<socket>` on the target
8. Exec the target (PID 1 becomes the target)
9. On exit: namespace dies, kernel sends SIGKILL to dbus-daemon + secretsd

Estimated: ~120-150 lines (more than originally estimated due to process
supervisor logic, readiness detection, and fd management).

### NixOS module

- New `libsecret` boolean option on wrapper definitions
- When enabled:
  - Sidecar config includes `secretsd_binary` and `dbus_daemon_binary` paths
  - Daemon config declares `<wrapper-name>/libsecret` as a mutable secret
  - Wrapper config sets `libsecret: true`
- `dbus` package path available for the namespace
- `underwrap-secretsd` binary installed alongside other binaries

## Storage model

Each libsecret-enabled wrapper stores its entire Secret Service state as a
single secret in the backend:

```
secret ID:  <wrapper-name>/libsecret
value:      {"collections":{"default":{"label":"Default","items":{...}}},...}
```

The envelope is a JSON object containing collections, items (with their
attributes, labels, timestamps), and secret values. It is read/written
atomically through the secretsd channel → daemon → StorageBackend.

This gives every wrapper its own isolated Secret Service state. A `gh`
wrapper's libsecret items are completely separate from a `slack` wrapper's.

## Bootstrap flow example

```bash
# 1. User runs the wrapper
$ work-gh-authorized auth login

# 2. underwrap-exec authenticates to daemon (GID), gets secret + secretsd channel fd
# 3. underwrap-ns creates PID namespace, launches dbus-daemon + secretsd
# 4. secretsd reads existing libsecret state from daemon via channel (or starts empty)
# 5. gh does OAuth flow in the browser, gets a token
# 6. gh calls libsecret CreateItem to store the token
# 7. secretsd updates in-memory state, writes JSON envelope to daemon via channel
# 8. Daemon writes envelope to StorageBackend (encrypted/remote/plain)
# 9. Done — token is in the underwrap backend, encrypted at rest

# Next time:
$ work-gh-authorized pr list
# secretsd reads envelope from daemon via channel on startup
# gh calls libsecret SearchItems + GetSecret
# secretsd serves from in-memory state (already loaded)
# gh gets the token, works normally
```

With remote backend: the envelope is stored on the operator's workstation
(via SSH-tunneled backend), never on the target machine's local disk.
This works automatically — no special handling needed, because all
storage goes through StorageBackend.

## Testing

### Unit tests (underwrap-secretsd-store)

- Create/delete collections
- Create/delete items with secret values
- Attribute search (exact match, subset match)
- Replace semantics on CreateItem
- Alias resolution (default alias)
- JSON envelope roundtrip (to_json / from_json)
- Empty store has default collection

### Unit tests (daemon secretsd channel)

- socketpair creation and fd passing via SCM_RIGHTS
- Channel handler scoping (rejects wrong secret IDs)
- Channel handler read/write round-trip
- Channel cleanup on EOF

### Integration test

- Start `dbus-daemon` + `underwrap-secretsd` on a temp socket with a
  mock channel (in-process socketpair, no real daemon needed)
- Use `secret-tool` (from libsecret) or the `secret-service` Rust crate
  as a client
- Store a secret with attributes
- Search and retrieve it
- Verify state persists through the channel (mock verifies write call)
- Verify no local files contain secret material

### VM test

- Configure a wrapper with `libsecret = true`
- Run `secret-tool store --label "test" service gh account github.com`
  via the wrapper
- Run `secret-tool lookup service gh account github.com` via the wrapper
- Verify the secret roundtrips
- Verify the secret is in the backend storage (daemon's StorageBackend)
- Verify NO local JSON files contain secret material
- Verify no D-Bus traffic leaks to the system session bus
- Verify secretsd is non-dumpable (`/proc/<pid>/fd/` not accessible)
- Verify the secretsd channel fd is not held by the target process

## Estimated effort

| Component | Lines | Complexity |
|-----------|-------|------------|
| `underwrap-secretsd-store` (state lib) | ~400 | Low — JSON CRUD |
| `underwrap-secretsd` (D-Bus server) | ~1800 | High — D-Bus protocol |
| `underwrapd` changes (channel) | ~250 | Medium — socketpair, SCM_RIGHTS, handler |
| `underwrap-ns` changes | ~150 | Medium — process supervisor, fd mgmt |
| `underwrap-exec` changes | ~50 | Low — SCM_RIGHTS receive, fd passing |
| NixOS module changes | ~40 | Low — add option, package, secret decl |
| Tests | ~400 | Medium |
| **Total** | **~3100** | |

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
