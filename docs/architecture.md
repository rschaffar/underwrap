# Architecture

underwrap separates durable resource and policy authority from local target execution.
The backend runs on Linux; the privileged frontend is supported through the NixOS module.
For the claim and its preconditions, read [Security](security.md).

## Components and identities

| Component | Responsibility |
| --- | --- |
| `underwrap-backend` | Resources, policy, selected storage backends, runtime and admin HTTPS APIs, and embedded admin SPA |
| `underwrapd` | One broker per frontend instance; authenticates entry-stage peers and forwards runtime requests |
| `underwrap-exec` | Setgid entry stage; obtains the daemon-selected execution plan and transfers the authenticated resource channel |
| `underwrap-ns` | Authenticates the handoff, resolves target identity, supervises namespaces, projects the resource, and attempts finalization |
| `underwrap-secretsd` | Invocation-private, in-memory Secret Service for libsecret-enabled targets |
| `underwrap-debug-tool` | Deliberately unsafe target available only through backend-authorized debug mode |
| `underwrap-keygen` | Installs a frontend runtime identity and outputs its public certificate for enrollment |
| `underwrap-info` | Unprivileged local discovery from public Nix-generated metadata, without executing targets or contacting the backend |

A **frontend instance** has its own daemon account, instance group, socket, setgid entry point, service, and private combined mTLS identity file.
A **runtime client instance** is the backend principal selected by an active enrolled leaf-certificate fingerprint and mapped to exactly one store.
A **wrapper/resource pair** binds a wrapper name to a resource; a **permission row** stores individual settings for that pair and one runtime client instance.
The backend's [client-wide override](backend.md#client-wide-override) determines when those saved settings apply.
Every local `(instance, wrapper)` pair has a dedicated target UID/GID, shared by its real and debug variants but not by other wrappers.

The local Unix caller is not a backend principal.
`allowedUsers` installs convenient PATH shims; any local user can invoke the system-wide instance entry point.
The caller is untrusted with respect to the resource, while the configured target is trusted with its entire selected projection.
Target-controlled protocol input is nevertheless validated for robustness and stored-state integrity.

## One invocation

```text
operator browser ── HTTPS/session/CSRF ──► underwrap-backend
                                               ▲
                                               │ HTTPS/mTLS
                                               ▼
caller ──► underwrap-exec ── instance socket ──► underwrapd
                 │                             │
                 │ execution metadata          │ scoped resource channel
                 ▼                             ▼
              host underwrap-ns ──► namespace PID 1 relay
                                           └─► PID 2 target
                                               ├─ private dbus-daemon
                                               └─ underwrap-secretsd
                                                   (when enabled)
underwrap-backend ──► selected StorageBackend
```

1. The setgid `underwrap-exec` entry stage—not the target—connects to the instance socket and supplies the wrapper selector and invocation variant.
2. The daemon verifies peer effective GID, resolves the wrapper's configured resource, and asks the backend to open the invocation.
3. The backend derives the current principal, store, and client override from mTLS-backed registry lookup, evaluates registered-pair access and any required permission row, and returns the resource and effective writable flag.
4. The daemon completely parses the decoded resource, conditionally validates env names, chooses the configured real target or authorized debug tool, and caches the original decoded bytes for one scoped channel.
5. The entry stage drops its instance-group privilege and hands the namespace stage execution metadata, the authenticated channel, and a separate parent-liveness pipe.
6. The namespace stage authenticates the channel before trusting metadata, reads target identity only from the root-owned table, and plans projections before their filesystem effects.
7. A trusted namespace PID 1 relay supervises the dedicated-UID PID 2 target and tears down its descendants before host collection.
8. With valid lifecycle evidence, the host compares the resulting canonical resource with Checkout and attempts usage reporting or writable Checkin.
9. The backend re-evaluates policy and writability before accepting persistence.

Finalization is diagnostic: valid target exit codes and signals remain the caller-visible result, not a persistence receipt.
A refused Checkin can be followed by UsageSignal, so the channel is not a single-terminal-verb or exactly-once protocol.
The [local protocol](local-protocol.md) owns descriptor, lifetime, parsing, projection, and publication details.
The [backend reference](backend.md#runtime-api) owns policy and stateless finalization.

## Why these boundaries exist

**Two entry stages separate privilege lifetimes.**
The setgid stage needs the instance group only to reach the daemon; the namespace stage needs capabilities for namespaces, mounts, and the dedicated identity switch.
The world-executable namespace wrapper therefore authenticates its fixed resource descriptor before privileged setup rather than trusting argv or metadata alone.

**Dedicated UIDs protect host-side process state.**
Namespaces provide private views and process-tree teardown, but do not by themselves protect host-side `/proc` reads.
One target UID per wrapper isolates resource trust domains even when wrappers execute the same binary.
Yama supplies additional ATTACH-mode protection, not the READ-mode boundary.

**One resource keeps related state together.**
Env, files, and libsecret sections form one bounded canonical value.
A wrapper selects projections rather than individual backend storage keys, and disabled sections survive writeback.
Atomic replacement avoids partial-value visibility, but does not make concurrent load-modify-save operations transactional.

**Writable staging is separate from write permission.**
Applications can perform ordinary files and Secret Service mutations even under a read-only permission.
The writable flag controls persistence, not target behavior or disclosure.

**Private publication avoids pathname authority.**
Secret Service state stays in memory and reaches the host through a bounded validated stream, not a target-owned final-state file.
Local acknowledgement permits candidate state to become live; it does not acknowledge backend durability or delivery to the D-Bus caller.

**Strict configuration rejects ambiguity before privileged use.**
Selectors and paths are validated rather than silently normalized.
Resource names have a separate backend contract; lexical canonical paths are not a `realpath` or symlink-resolution guarantee.
See [Configuration](configuration.md) for exact grammar, overlap domains, and shim collisions.

## Runtime-client scheduling

Each daemon shares one asynchronous reqwest client across registration, open, and finalize, with idle connection pooling disabled.
A fail-fast semaphore admits at most 32 calls with no pending queue or global FIFO worker.
DTO construction and finalize base64 encoding can precede admission, but rejection does not invoke the request closure, construct a RequestBuilder, or start network work.
Saturation follows the call site's existing warning or internal-error path.

One 30-second deadline starts after admission and includes sending and complete success or error decoding.
Timeout or cancellation releases the local permit but cannot establish that the backend made no change.
There is no automatic retry, rollback, deduplication, or exactly-once delivery.
Each scoped channel processes requests sequentially; independent invocations may finish out of order.
This bounds local queueing, not availability when all permits are occupied or the backend is unreachable.

## Deployment and compile boundaries

The NixOS module provisions root-owned configuration, users/groups, target identity and runtime directories, security wrappers, services, and host-posture assertions.
It is part of the security architecture, not merely packaging.
Deploy entry, daemon, and namespace binaries together through one package/module generation; mixed local protocol binaries are intentionally incompatible.

`underwrap-protocol` defaults to `full`, which includes configuration, policy, service/schema, asynchronous IPC, and SCM_RIGHTS support.
`underwrap-ns` disables defaults and selects `local-abi`, retaining the always-compiled resource, execution, publication, message, target-environment, and synchronous IPC surface.
An isolated namespace package build excludes full-only protocol dependency edges while retaining the namespace crate's direct libc dependency.
This narrows compile and review coupling, not demonstrably the linked binary, runtime trusted computing base, attack surface, or vulnerability set.
Workspace feature unification can include `full`; use the isolated checks described in [Testing](testing.md).

For deployment and maintenance, use [Operations](operations.md).
For helper design and explicit host access, use [Wrappers](wrappers.md).
