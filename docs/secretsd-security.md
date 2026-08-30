# secretsd security

`underwrap-secretsd` implements `org.freedesktop.secrets` on the private D-Bus
bus used by a libsecret-enabled wrapper.

The configured target is trusted with its projected libsecret data, but every
D-Bus message remains untrusted input for process robustness, resource bounds,
object-path integrity, and canonical backend state.

## Data flow

secretsd does not connect to `underwrapd` and never receives fd 20.

```text
underwrapd ◄──────── scoped resource channel fd 20 ───────► host underwrap-ns
                                                               ▲
                            private publication socket          │ validate,
                    ACK/REJECT each exact generation            │ retain,
                                                               │ finalize
namespace PID 1 ── PID 2 setup ── underwrap-secretsd ──────────┘
                          │               ▲
                          │ init pipe     │ private D-Bus
                          ├──────────────►│
                          ├─ dbus-daemon  │
                          └─ target ──────┘
```

1. `underwrap-ns` authenticates fixed fd 20 and checks out the complete
   canonical resource blob.
2. After Checkout establishes that libsecret is enabled and before the PID
   namespace fork, it creates one invocation-private CLOEXEC Unix stream
   socketpair. The host owns the receiver and secretsd becomes the sole
   long-lived publisher.
3. PID 2 sends the initial libsecret section through a separate one-shot pipe.
   secretsd loads it into memory, publishes generation 0, and waits for complete
   host `ACK(0)` before reporting readiness. The target cannot execute first.
4. Every D-Bus mutation keeps the store write lock, clones the accepted store
   once, mutates that candidate, canonicalizes and publishes the next complete
   attempt generation, and commits the candidate only after observing the exact
   generation-matched host ACK. Preparation or canonicalization failure, host
   `REJECT`, framing or sequencing failure, EOF, response mismatch, and transport
   failure return `Failed` and preserve the accepted live store.
5. The host incrementally bounds, parses, exact-canonical-byte checks, and
   semantically validates each generation. It retains a candidate before writing
   ACK and selects it as latest only after the complete ACK is written.
6. Namespace PID 1 reaps direct PID 2, frames its exact status, and exits so the
   kernel terminates secretsd, dbus-daemon, descendants, and other helpers. The
   host requires terminal publication EOF after clean relay reap.
7. Libsecret collection and a terminal resource verb require all four facts:
   exact `Armed`, one valid direct-target status, clean relay reap, and terminal
   publication EOF with at least ACK-written generation 0.

The host's ACK means invocation-local validated retention. It does not prove
that the D-Bus caller received its response or that the backend committed the
resource, and a later Checkin can still fail. secretsd can die after the host
completely writes ACK and before secretsd or the caller observes success; that
ACK-written generation remains eligible after terminal EOF. A later ordinary
rejected, partial, response-failed, or publisher-death attempt also preserves the
previous ACK-written generation. A final nonblocking `EAGAIN` is different: it
proves a publisher endpoint remains open, so no generation fallback, collection,
Checkin, or UsageSignal is allowed.

The authenticated resource channel has a dedicated 2-MiB complete-resource
bound plus its message envelope; unrelated control IPC remains capped at 64 KiB.
A private publication near the 1-MiB cap is eligible for Checkin when its
composition with Checkout env/files remains within the complete-resource cap; the
ACK is not a backend-durability promise.

## Descriptor ownership

The socketpair is created atomically CLOEXEC. The namespace branch closes its
receiver immediately and the host closes its publisher immediately. PID 1 drops
its publisher copy immediately after forking PID 2. PID 2 keeps its copy only
while forking secretsd and drops it before readiness, `Armed`, or target exec.
dbus-daemon explicitly closes it.

The secretsd exec child is the one reviewed CLOEXEC exception: it clears
CLOEXEC only on its publication, init, and readiness descriptors for that exec.
secretsd becomes non-dumpable, adopts the publication descriptor through RAII,
and immediately restores CLOEXEC before reading initialization bytes. It closes
the init and readiness descriptors after their one-shot roles. The target,
descendants, namespace PID 1, dbus-daemon, and other helpers retain neither
publication endpoint. `close_range(..., CLOEXEC)` remains a final backstop.

## Host validation and bounds

A publication request carries fixed magic, version, `PUBLISH` tag, big-endian
`u64` attempt generation, big-endian `u32` body length, and exactly that body.
Responses carry fixed magic/version, exact `ACK` or `REJECT`, and the matching
generation. Generation 0 is mandatory; every sent attempt consumes the next
generation. Replay, gap, overflow, malformed framing, over-cap declaration, and
transport truncation poison the stream rather than resynchronize.

The body cap is inclusive at 1,048,576 bytes and is checked before allocation.
For every host-selectable generation, the host requires:

- parse as `LibsecretSection` and byte-identical JCS re-encoding;
- collection ids and alias names that are nonempty, at most 128 bytes, and only
  ASCII letters, digits, or underscore;
- item ids that are exactly 32 lowercase hexadecimal bytes;
- alias targets that are valid existing collection ids;
- the exact derived one-segment collection, item, and alias path shapes;
- every collection/item `created` and `modified` at most `2^53 - 1`.

Labels, content types, attribute keys/values, and secret bytes remain arbitrary.
Loaded timestamps may have arbitrary temporal relationships within the protocol bound, with no created-before-modified, future-time, or collection/item temporal rule.
A successful `Item.SetSecret`, `Item.Attributes`, or `Item.Label` mutation samples one Unix second and assigns that exact value to only the item and containing collection `modified` fields.
The candidate carries those values during publication, but they enter accepted live memory only after the host ACKs that exact generation.
The sampled value has one-second resolution and no monotonic guarantee.
A rejected or failed candidate is discarded without changing accepted live memory, so later mutation candidates start from the previously accepted state.

## Process hardening

secretsd calls `prctl(PR_SET_DUMPABLE, 0)` before adopting the publication
capability or reading initialization secrets. Its environment is cleared by
`underwrap-ns`; only a small trusted helper allowlist is restored.
Caller-controlled `DBUS_*`, `XDG_*`, and dynamic-linker variables do not select
the private service.

The process runs under the wrapper's dedicated target UID inside private PID
and mount namespaces. The target-facing tmpfs contains the filesystem D-Bus
socket and helper logs, but no final-state pathname used by host writeback.
Target-caused indefinite D-Bus stalls remain a denial-of-service non-goal.

## Handler behavior

Collection creation and `SetAlias` share one segment rule that accepts exactly 1 through 128 bytes of ASCII letters, digits, or underscores, including a leading digit or underscore.
Item identifiers are generated as 16 random bytes encoded to 32 lowercase hexadecimal characters.
Object paths are constructed fallibly.

`SetAlias` validates the alias first and then accepts only `/` for clear or the exact `/org/freedesktop/secrets/collection/<one-valid-segment>` shape for set.
Malformed aliases and paths return value-free `InvalidArgs` before the store lock and before candidate publication.
A set operation acquires the write lock and rejects a nonexistent target with value-free `InvalidArgs` before candidate publication, so the existence check and later mutation observe the same accepted store state.
Root clear, clearing a missing alias, and setting an alias to its existing target remain valid idempotent mutations through publication and ACK.
Strict host publication validation remains defense in depth against malformed segments and dangling references from any publication producer.

Mutation handlers retain the existing write lock through clone, candidate mutation, canonicalization, publication, exact ACK observation, and commit.
They clone the accepted store exactly once and expose the candidate as live state only after ACK.
Preparation, canonicalization, rejection, response, EOF, and transport failure discard the candidate and preserve accepted live memory.
Candidate secret buffers are zeroized when discarded; on commit, secret buffers displaced from the former live store are zeroized, including deletion and replacement values.

`Collection.CreateItem` defaults an absent Item.Label to the empty string and absent Item.Attributes to the empty map.
A present `org.freedesktop.Secret.Item.Label` is admitted only when its outer D-Bus type is exactly `s`, and a present `org.freedesktop.Secret.Item.Attributes` only when its outer type is exactly `a{ss}`.
A nested variant is not treated as either exact type, and unknown properties remain ignored.
The handler validates Label first and Attributes second, so Label has deterministic error precedence when both are malformed.
A wrong Label type returns `InvalidArgs("org.freedesktop.Secret.Item.Label must have D-Bus type s")`.
A wrong Attributes type returns `InvalidArgs("org.freedesktop.Secret.Item.Attributes must have D-Bus type a{ss}")`.
Both fixed value-free errors occur before secret metadata tracing, the store lock, candidate or random-id creation, publication, or object registration.
Valid requests retain subset attribute replacement, including vacuous matching for absent Attributes, and ACK-gated candidate commit semantics.

## Backend authorization

secretsd does not decide backend writability. With the four-way lifecycle
conjunction, unchanged state sends UsageSignal, changed state from a read-only
Checkout is discarded with the documented audit/warning and sends UsageSignal,
and changed writable state attempts Checkin. The backend re-evaluates permission
and writability. Finalization errors remain diagnostic and do not replace valid
target exit or signal status.

## Related documentation

- [Architecture](architecture.md#libsecret-projection)
- [Security](security.md#libsecret)
- [GitHub via libsecret](gh-libsecret.md)
