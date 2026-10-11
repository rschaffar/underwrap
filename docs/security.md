# Security

underwrap is pre-release security software.
It has not been formally verified or independently audited end to end.
The following is a scoped engineering claim, not proof of defect-free implementation or release readiness.
For confidential reporting, see [SECURITY.md](../SECURITY.md).

## Threat model

**The caller is untrusted with respect to the brokered secret.**
This includes other non-root processes owned by the caller.
Assume callers can invoke any system-wide instance entry point, select configured wrapper names and arguments, control their initial environment and inherited stdio, inspect ordinarily visible metadata, race invocations, and submit malformed reachable IPC.

**The configured target is trusted with its projection.**
It can print, persist, or transmit that projection through its own output, files, network, plugins, helpers, or application behavior.
underwrap is not a behavioral sandbox.
Target-supplied D-Bus input remains untrusted for process robustness, bounds, object-path integrity, and canonical stored state; see [Secret Service input](local-protocol.md#secret-service-input).

**Local Unix users are not backend principals.**
Backend policy identifies the mTLS-authenticated runtime client instance and wrapper/resource pair, not the calling Unix user.
`allowedUsers` installs PATH shims only; the setgid entry point is system-wide.
The entry gate enforces traversal through trusted code, not per-user authorization.
Use an OS-level gate such as sudo or polkit when local-user authorization or a prompt is required.

**The backend, configured storage implementation, host root, root-owned configuration, and Linux kernel are trusted.**
The design also depends on NixOS security-wrapper behavior and TLS, cryptographic, and serialization dependencies.
Compromise of these authorities is outside the local caller-confidentiality claim.

### Assets and compromise directions

| Asset | Consequence of compromise |
| --- | --- |
| Resource values and private projection/channel capabilities | Disclosure of the authorized resource; fd 20 also conveys a scoped broker capability |
| Runtime client's private identity | Impersonation of that runtime client instance |
| Active or retained ciphertext plus its generation key | Disclosure of current or stale values in that generation |
| Backend server private key | Backend impersonation |
| CA signing key | Unauthorized issuance inside that CA trust domain |
| Admin password verifier | Offline password guessing and possible operator compromise |
| Trusted configuration integrity | Unauthorized target, wrapper, or backend behavior changes |

Names, mountpoint names, deny reasons, and ordinary process metadata are not protected as secrets.
Nevertheless, operator-supplied identifiers can contain sensitive text: do not put sensitive content in names or copy deletion blocker samples into logs.

## Hardened claim and preconditions

With all the following conditions, there is no known kernel-mediated path for a non-root local caller to read raw brokered values from the wrapper or target process tree:

- The frontend uses the NixOS module with `services.underwrap.hardenTargetHost = true` and Linux 6.5 or newer.
- `kernel.yama.ptrace_scope = 2` is effective; the module supplies an overridable `mkDefault`, not an unchangeable setting.
- Swap is absent, ordinary swap meets the module's random-key/LUKS assertion, or zram is RAM-only for memory swap.
- Root-owned wrappers, configuration, target-UID table, accounts, capabilities, and services retain their trusted integrity.
- The daemon trusts the correct backend server certificate and only its instance account can read its runtime private identity.
- The configured target is a narrow reviewed helper whose accepted arguments, inputs, descendants, and downstream credentials do not let the caller cause disclosure.

The target condition is load-bearing: kernel controls cannot prevent a trusted recipient from revealing its own secret.
See [Wrappers](wrappers.md) for reviewing that boundary.

The module rejects typed zram-generator swap writeback because a backing-device path does not establish encryption.
Canonical mounted zram filesystem sections are not memory swap, and canonical non-zram sections are outside that swap predicate.
When zram-generator is enabled, all final typed section/directive names must still be canonical tokens and every string value single-line, including mounted and non-zram sections.
These are configuration checks, not inspection of external drop-ins or live devices.
Operators must preserve the posture outside module-managed configuration; exact predicates belong in [Configuration](configuration.md).

Yama restricts unprivileged debugging of ordinary host processes, including `strace`, `gdb`, ptrace attachment, and `process_vm_readv`.
The dedicated target UID is the primary READ-mode `/proc` boundary for `environ`, `maps`, and related entries; Yama adds ATTACH-mode protection and does not protect `environ` reads.
World-readable metadata such as command lines remains visible, which is why secret material must never travel in argv.

Disabling `hardenTargetHost` explicitly selects a weaker posture.
Only unsupported `SO_PEERPIDFD` (`ENOPROTOOPT`) selects the PID-based peer fallback; errors on the pidfd path reject instead of downgrading.
The supported hardened deployment's kernel floor excludes that fallback.

## Local guarantees and their limits

The [local protocol](local-protocol.md) is the detailed owner of the mechanisms below.

| Boundary | Mechanism and limit |
| --- | --- |
| Caller → daemon | Effective-GID socket gate, kernel peer identity, validated selector, backend authorization; neither wrapper name nor `argv[0]` is authority |
| Entry → namespace supervisor | Fixed fd-20 socket authentication before fd 3 or setup; fd 21 supplies liveness only |
| Caller → target process state | Dedicated target UID/GID, checked permanent identity and capability drop, private namespaces, core limits; target exec may reset dumpability |
| Process-tree lifetime | Active relay liveness observation and namespace-init exit; teardown is eventual, not graceful signal forwarding or instantaneous death |
| Target → broker | The configured target receives neither resource nor publication channel; private helpers receive only their assigned endpoints, never fd 20 |
| Projection → persistence | Exact lifecycle evidence and backend reauthorization; local ACK and target status are not persistence receipts |

Real and debug variants share a target UID because they share the wrapper's resource trust domain.
Target stdio remains connected to the caller.
A fresh session, optional anonymous session keyring, and descriptor cleanup reduce accidental reach back, but do not create an adversarial-target sandbox.
`accessPaths` deliberately grants selected host filesystem access and target writes there are caller-owned on the host.
Mount namespaces hide path-based buses, not abstract Unix sockets.

Setup failures normally suppress execution or finalization rather than silently weakening the active posture.
Important qualifications are explicit in [Lifetime and privilege](local-protocol.md#lifetime-and-privilege): stage-one supplementary-group `EPERM`, the files supervisor's retained capabilities, best-effort failed-entry fsgid rollback, and the narrow post-Armed unchanged-state ambiguity.
Cancellation before dispatch sends no terminal verb, but later parent death cannot retract a completed backend request.

## Persistence and memory domains

Underwrap-managed durable brokered resource material uses the backend's selected `StorageBackend`.
The frontend retains it in process memory, except files staged on invocation-private `noswap` tmpfs.
Retained directory handles, rather than a shared `/tmp` staging pathname, select the same private filesystem objects for materialization, binding and collection.
Libsecret state stays in memory; its bus tmpfs carries sockets and helper artifacts without a `noswap` claim.
Ordinary temporary secret files, frontend disk caches, sidecar databases, and alternate durable frontend resource paths are not part of the design.
A trusted target can independently persist or disclose its own projection.

Migration and rotation can create bounded-purpose backend-host sibling workspaces outside active resource APIs.
Migration removes its workspace before success or requires recovery; rotation retains old-key-readable generations indefinitely until trusted-host cleanup.
Deletion and key rotation are not secure erasure, and retained generations have confidentiality and unbounded disk-cost consequences.
See [Store lifecycle](backend.md#store-lifecycle) and [Operations](operations.md) for generation custody, cleanup, and recovery.

Runtime identities, server/CA keys, password verifiers, trusted configuration, and externally held generation keys are separately managed sensitive domains.
Nix-generated configuration protects integrity, not secrecy: `envSet` must contain only non-secret fixed values.

Typed secret vectors are wiped where practical, not comprehensively.
HTTP/base64 buffers, original decoded cache, Checkout/frame/receiver copies, serialization, dependencies, allocators, sockets, and OS memory are outside a complete erasure guarantee.
Lock drops the registry's backend reference; `MasterKey` zeroization occurs on final reference release, which admitted leases can delay.
It does not erase every request, response, browser, clipboard, or process copy.
See [Parsing and copies](local-protocol.md#parsing-and-copies).

## Backend transport and administration

The daemon requires an absolute HTTPS URL with a host and normal hostname verification.
Startup rejects invalid URLs before credential reads or socket binding, client construction repeats validation before file I/O, and HTTPS-only reqwest transport prevents redirect downgrade.
A configured CA bundle is its exclusive server trust set; without one it uses bundled WebPKI roots.
This server-authentication choice is separate from runtime identity: the backend requires private-key possession and an active enrolled leaf fingerprint, not subject, SAN, CA chain, KU/EKU, or validity dates as principal selectors.
Direct TLS and Layer-4 passthrough preserve that peer identity; ordinary Layer-7 TLS termination with forwarded HTTP does not, and no forwarded-certificate trust mechanism is provided.

Runtime bodies remain parser-untrusted after authentication and cannot select a store or storage key.
A daemon caches its combined identity when constructing the client; replacing it requires an explicit restart and interruption, not live reload.
[Identity installation](local-protocol.md#identity-installation) describes ownership and commit limits; [Operations](operations.md) owns enrollment and replacement.

The first operator and total-login-loss recovery are distinct offline tasks requiring backend-host authority, stopped competing writers, the service identity, and a trusted password line on stdin.
The CLI does not supply a hidden terminal prompt.
Bootstrap's stable advisory lock coordinates participating bootstrap processes, not every trusted writer; it commits the user before its sentinel and does not roll back a valid user if sentinel persistence fails.
Recovery creates a distinct user without changing the sentinel and requires verification through ordinary login.
Do not delete registries or sentinels, or hand-edit verifiers, as recovery shortcuts.

Admin authentication uses Argon2id verifiers, opaque `__Host-underwrap-session` cookies with in-memory sessions, synchronizer-token CSRF, optional allowed-origin checks, per-username throttling, and dummy verification for unknown usernames.
Protected mutations require session and CSRF; public login is an exception, not a CSRF-authenticated mutation.
All operators have full administration rights, with a last-user deletion guard rather than roles or per-store grants.

### Sessions and browser custody

Sessions disappear on backend restart.
A self-service password change persists the new hash before refreshing the submitting session; refresh failure still means the password changed and requires another login.
Other pre-change sessions, reset-user sessions, and deleted-user sessions are invalidated lazily on their next admin request.
Logout primes a fresh CSRF token and revokes backend session authority before clearing the SPA cache and navigating.
Failure preserves session/cache/navigation state with a visible retry, rather than reporting local-only success.

Generated create/migration keys remain in transient component state, outside application query/mutation caches, and require exact untrimmed re-entry before submission.
No-cache response headers and inspected application storage use do not establish absence from browser internals, intermediaries, clipboard, allocator remnants, screenshots, traces, or failure artifacts.
An ambiguous submission retains the same key while state is reconciled; rotation instead returns its key after mutation.
Neither browser comparison nor response success establishes external custody or independent recoverability, and underwrap supplies no TPM, HSM, or external-vault custody guarantee.
Use [Operations](operations.md) for the key and session procedures and [Testing](testing.md) for bounded browser evidence.

Backend server/CA keys and verifier records need deployment ownership, modes, umask, and custody controls.
The backend does not install or mode-check its configured TLS key, manage the CA key, enforce the user-registry directory mode, or encrypt verifiers at rest.
Restrictive temporary-file creation, file sync, and atomic replacement do not imply parent-directory fsync, power-loss durability, or secure-media erasure.

## Policy, outcomes, and diagnostics

[Backend policy](backend.md#permission-model) governs open and finalize independently.
The trusted operator can apply a backend runtime-client override without rewriting individual permission rows.
Against a caller seeking resource disclosure or unauthorized writeback, the boundary remains backend-controlled: Unlock all broadens ordinary registered-pair access but does not bypass identity, store, registration, validation, or env-integrity checks.
Unsafe debug still needs its separate active timed per-pair approval; Lock all denies both variants.
The current override is resolved per request, not supplied by the caller or cached as daemon configuration.
An override change neither recalls a delivered projection nor upgrades an existing read-only Checkout, and already admitted operations retain the documented concurrency limits.
Read-only files and libsecret staging remain writable to the trusted target; the writable flag only controls persistence.
Env comparison protects against changes relative to the snapshot loaded by finalize, not against concurrent admin edits between load and save.
Whole-resource and control-document mutations are stateless last-writer-wins operations; operators must serialize participating writers.
Resource deletion, registry publication, and cross-entity checks do not supply CAS, transactions, automatic repair, or post-delete exclusion.

Unsafe debug grants the complete configured projection to an intentionally broad target for the exact authorized runtime client instance, pair, and expiry.
Any local user can invoke it while authorized; read-only does not reduce its disclosure capability.
Confirming the UI's proposed expiry changes only the local form; Save submits that selection.
Existing permissions and client overrides remain effective until changed or expired.
Use the shortest practical interval, restore a safer mode, and apply an external OS gate if necessary.

Generic backend internal errors return HTTP 500, code `internal`, and `the backend could not complete the request`.
Generic internal events log static context, response code, and Rust error type, never arbitrary source display text.
Detailed source text is restricted to individually audited filesystem, storage, canary, and registry error classes; new source classes require their own material-flow audit.
These fields classify operations, not uniquely correlate requests, and do not certify every framework or future log path.

Daemon audit helpers use human-readable tracing to stdout, not a JSON or durable audit ledger.
Namespace writeback rejection uses native journald with a JSON stderr fallback; suppressing the optional read-only warning never suppresses its audit event or oversized-resource diagnostic.
The exact event vocabulary and diagnosis workflow belong in [Operations](operations.md).
Keep values, resource blobs, credentials, keys, verifiers, certificate bodies, and blocker samples out of logs and evidence artifacts.

## Non-goals and evidence limits

underwrap does not provide protection against root/kernel/backend compromise, target disclosure, overpowered downstream credentials, denial of service, per-invocation human approval, universal availability, or a tamper-evident audit ledger.
It does not guarantee complete erasure, exactly-once persistence, response delivery, automatic recovery, or transactions.
Recovery cannot promise restoration after media loss, missing generation keys, arbitrary corruption, or partially completed cleanup.

Unit and typed-handler tests do not establish kernel behavior, complete D-Bus wire conformance, post-drop erasure, or target trustworthiness.
Nix evaluation is not activation; selected VM, journal, and browser witnesses are not universal deployment, sink, platform, or browser support evidence.
See [Testing](testing.md) for the actual commands, support declarations, witness boundaries, and required cross-component review.
