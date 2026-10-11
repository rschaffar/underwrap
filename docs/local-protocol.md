# Local protocol

This reference owns the privileged entry, handoff, process lifetime, parsing, projection, and Secret Service publication mechanisms.
Read [Architecture](architecture.md) for the component flow and [Security](security.md) for the caller-untrusted, target-trusted threat model.
These interfaces are internal and deployed together through the NixOS module; they are not independently version-compatible CLI extension points.

## Entry and authenticated handoff

Each frontend instance has a daemon account/group `underwrap-<instance>`, setgid entry `/run/wrappers/bin/underwrap-<instance>`, and `0660` socket `/run/underwrap-<instance>/wrapper.sock`.
The unprivileged PATH shim invokes that entry; it does not grant per-user backend authorization.

`underwrap-exec` derives the instance from its kernel-assigned effective group, not `argv[0]`, and loads socket/helper paths from the strict root-generated `/etc/underwrap/instances/<instance>.yaml` sidecar.
It connects to the daemon as the socket client and sends only the wrapper selector and real/debug variant.
The daemon checks effective GID rather than relying on `SO_PEERCRED`'s real GID alone.
On Linux 6.5+, it holds a `SO_PEERPIDFD` pidfd while reading peer process credentials, preventing PID reuse during lookup, then cross-checks identity.
Only unsupported-option `ENOPROTOOPT` selects the weaker `SO_PEERCRED` plus `/proc/<pid>/status` path, available on older kernels only outside the module's hardened posture.
Any acquired-pidfd lookup/read/cross-check failure rejects rather than falling back.

Before binding, daemon group lookup uses reentrant NSS state with caller-owned output/buffer lifetime through result-pointer validation and GID copy.
The sysconf size is only a hint: absent, zero, or negative uses 16 KiB; unrepresentable or over-1-MiB hints fail.
Only `ERANGE` grows by checked doubling up to 1 MiB; allocation failure, exhausted growth, `EINTR`, other errors, invalid input, or unexpected result pointers abort startup.
A zero result with null output is ordinary not-found, not another identity fallback.

The daemon rejects unknown wrapper names and the reserved `debug` wrapper token, resolves the configured resource, and asks the backend to authorize the requested variant.
Only afterward does it choose the real target or debug tool and create a CLOEXEC Unix stream socketpair scoped to that resource.
The caller's selector is data, never identity or target-path authority.

The daemon lends an endpoint to `sendmsg`; it retains ownership of its local descriptor.
The entry receiver uses `cmsghdr`-aligned storage and `MSG_CMSG_CLOEXEC`, immediately adopts every installed SCM_RIGHTS descriptor, and accepts exactly one rights message containing exactly one descriptor with no other control message.
Truncated, extra, malformed, or mixed control data causes all adopted descriptors to be dropped before rejection.
Owned CLOEXEC relocation above fixed slots prevents mapping one source over another; only deliberate `dup2` clears CLOEXEC on the handoff destination.

| Descriptor | Role |
| --- | --- |
| fd 3 | Descriptor-free execution metadata; no resource env values or target UID/GID |
| fd 20 | Fixed authenticated resource channel; not selectable by bundle data |
| fd 21 | Read-only stage-one parent-liveness pipe; no authority or metadata |

Because the capability-bearing namespace wrapper is world-executable, it authenticates fd 20 **before reading fd 3, trusting selectors/target argv, or privileged setup**.
It requires an `AF_UNIX` `SOCK_STREAM` socket whose creation-time `SO_PEERCRED` UID/GID matches the selected instance's root-provisioned daemon account.
Missing accounts/descriptors, wrong peer identity/type, root identity, or daemon/target identity overlap abort.
Socketpair creator credentials survive SCM_RIGHTS, duplication, fork, and exec; possession of legitimate fd 20 is itself the scoped capability and must remain confidential.
No bearer token in argv or environment substitutes for it.

Only then does the namespace stage validate fd 21, arm parent-death `SIGKILL`, and check for an already-dead parent.
The waiting entry process owns the sole writer until its namespace child is reaped.
The namespace host keeps the reader through a checked pre-fork rearm, then closes it.

## Lifetime and privilege

The setgid-only entry attempts to clear supplementary groups and tolerates `EPERM`, because it normally lacks `CAP_SETGID`.
It then checks permanent restoration of real, effective, and saved GID/UID to the caller's ids.
Caller supplementary groups can therefore survive stage one, but not the final dedicated-target switch.

Target UID/GID comes solely from the root-owned `/etc/underwrap/target-uids` table keyed by `(instance, wrapper)`.
Neither runtime API, daemon response, execution bundle, argv, nor caller environment supplies it.
Missing selectors, table, row, user, or group are fatal, with no caller/root fallback.
Real and debug variants share their wrapper identity; different wrappers and instances do not.

The namespace stage becomes non-dumpable before handling secrets.
After authenticating fd 20, an optional secret-free idmap helper runs and is reaped before fd 3 or Checkout is read.
The caller-sanitized need-idmap hint must agree with the decoded bundle.
The helper closes fd 3 and fd 20 before briefly becoming dumpable for its proc map setup; the parent remains non-dumpable.

With fixed handoff slots still occupied, the host creates private CLOEXEC authorization-gate, inner-liveness, and target-status pipes.
It reads execution metadata, checks out the resource, builds the final environment, and completes pure files planning before namespace preparation, idmapped mounts, PID fork, staging, target identity switch, or target-facing helper launch.

The host unshares PID, mount, IPC, and UTS namespaces, makes mount propagation private, and realizes planned host-access mounts.
For files projection, it creates one capped `noswap` tmpfs inside a checked target-fsid window before fork.
The filesystem is created detached, then attached by descriptor to a mode-`0000` `.underwrap-files` directory beneath the dedicated target's provisioned mode-`0700` runtime directory, only in this private namespace.
Both attachment directories require exact target UID/GID and modes on their held descriptors, interpreted in the current mount/idmap view; symlinks and unexpected visible ownership reject.
This namespace attachment is required before `open_tree` can clone the projection directories.
Retained CLOEXEC projection handles pin the same filesystem objects in both supervisors; no files-staging mountpoint lives in shared `/tmp` and no later staging operation re-resolves the runtime pathname.
Filesystem-context creation, options, mount creation, attachment and empty-directory creation all run as the target filesystem identity, with checked restoration before the final pre-fork liveness rearm.
Unsupported mount APIs or required options reject without a pathname-based fallback.
The host then forks a trusted non-execing PID 1 relay.
The relay remounts `/proc`, switches supplementary groups to only the target group and all real/effective/saved ids to the dedicated identity, and becomes non-dumpable before staging.
It arms parent-death `SIGKILL` and rearms immediately after credential switching, which clears that setting.
After projection setup it drops its staging handles before any target/helper fork, sets `NoNewPrivs`, clears effective/permitted/inheritable/ambient capabilities, verifies the drop and inability to regain UID 0, and performs a final liveness arm/check.
It closes fd 20 before forking the configured target as PID 2.

PID 2 establishes its own session, restores the intended SIGCHLD state, starts private helpers, and closes or marks CLOEXEC every fd above stderr before target exec.
It receives no resource channel, publication endpoint, relay liveness pipe, or target-status writer.
The target keeps stdio, receives soft/hard `RLIMIT_CORE=0`, and joins a fresh anonymous session keyring when available.
Exec can reset dumpability, so the dedicated UID remains the confidentiality boundary.

PID 1 retains and actively polls the inner sole-writer liveness pipe while waiting for PID 2.
Any pipe event exits fail-closed; this active observation is authoritative and parent-death signals are defense in depth.
PID 1 reaps PID 2, performs a final liveness check, writes one exact direct-target exit/signal frame, and exits so the kernel terminates helpers and detached descendants.
There is no graceful signal forwarding.
Teardown is eventual and can be delayed by scheduling or uninterruptible sleep; external root-held namespace references are outside the claim.

### Finalization evidence

PID 2 is the authorization-gate reporter: it reports `Armed` after final descriptor protection, while a controlled return reports `Failed`.
Temporary helper copies close on exec or process exit, as described under [Secret Service publication](#secret-service-publication).
The host observes the gate, cleanly reaps PID 1, and parses the target-status pipe nonblockingly.
Collection and terminal resource dispatch require:

- Exact `Armed` followed by EOF, without `Failed`.
- One valid direct-target status frame followed by EOF.
- A clean relay reap.
- For libsecret, terminal publication EOF and at least validated generation zero whose ACK was completely written.

Early EOF, malformed/truncated/extra lifecycle input, retained writers (`EAGAIN`), I/O failure, failed wait, or an unclean relay suppress collection and both Checkin and UsageSignal.
A valid target status still propagates when gate or publication evidence suppresses resources.
The gate proves an authorized lifecycle, not successful exec.
In the narrow interval after `Armed` but before either CLOEXEC exec closure or complete observation of `Failed`, child death is indistinguishable from exec closure.
If no target ran in that interval, finalization can include at most the already validated, fully materialized unchanged projection, not arbitrary pre-exec partial state.

Cancellation established before dispatch sends no terminal verb.
A request completed before later stage-one death cannot be retracted without backend transactional support.

### Supervisor capability and fsid window

The NixOS namespace wrapper supplies `CAP_SYS_ADMIN`, `CAP_SETUID`, and `CAP_SETGID`.
`CAP_SETPCAP` is named so the security wrapper can raise those capabilities; NixOS does not pass it into `underwrap-ns`'s ambient set.
Supervisors without files projection drop capabilities before waiting.
Files supervisors use a target-fsid window for detached staging setup and retain all three capabilities through target execution and the bounded post-run files-only fsuid collection window; they do not selectively discard `CAP_SYS_ADMIN` before waiting.

Ordinary fsuid/fsgid restoration is queried and checked, and restoration failure is fatal with the collected result dropped.
Failed entry is different: if setting fsuid fails after fsgid changed, the fsgid rollback is best effort and its result is not verified before returning the fatal setup error.
Do not treat general fail-closed wording as proof that this branch has checked rollback.
The implementation is [`target_uid.rs`](../crates/underwrap-ns/src/target_uid.rs).

## Parsing and copies

Backend-open success first buffers the complete HTTP body and deserializes owned strings, including `blob_b64`, into ordinary non-zeroizing allocations.
There is no application-level HTTP response-body cap; non-success error text is likewise uncapped.
The daemon's admitted-call deadline covers this decoding, not just response headers.

The daemon standard-base64-decodes the complete outer string before checking the decoded resource size.
`ResourceBlob::from_canonical_bytes` enforces the inclusive complete-resource cap, then parses the full typed JSON, allocates metadata, decodes every secret into `SecretBytes`, and bounds all present collection/item timestamps before checking schema version 1.
It performs this traversal even when env projection is disabled.

Unknown response/resource/nested fields are ignored; known duplicate struct fields are rejected.
Duplicate keys inside map-valued objects are not independently rejected and later entries replace earlier ones; defaulted omitted sections become empty.
Daemon open validates env names only when env projection is enabled.
It does not compare input bytes with JCS reserialization, perform files routing/projectability checks, or enforce strict libsecret identifiers, references, composition, or temporal relationships beyond safe integers.

The channel caches the **original decoded bytes**, not base64 or a normalized typed reserialization.
Accepted noncanonical syntax, ignored fields, defaulted omissions, and duplicate map keys therefore remain in that cache.
The temporary typed resource lives through channel-context assembly alongside the outer base64 string and cache, then its parsed `SecretBytes` zeroize on drop when open returns.

Checkout copies the cache into `CheckoutOk.blob`; wire encoding copies it into a frame payload, and the receiver copies the inner blob after bounds checking.
The sender's cache copy precedes wire revalidation of the already capped bytes.
Cache, response/frame vectors, namespace original bytes, metadata, dependency/allocator/OS/socket copies, and outer HTTP/base64 storage are not comprehensively zeroized.
These ownership distinctions do not imply the backend is untrusted or that typed parsing proves exact JCS.
Backend finalize's separate validation is described in [Backend](backend.md#finalize-and-concurrency).

### Independent size domains

| Domain | Inclusive byte limit |
| --- | --- |
| Complete canonical resource | 2,097,152 |
| Canonical libsecret publication body | 1,048,576 |
| Ordinary control IPC payload | 65,536 |
| Checkin resource payload | Complete resource plus 5 bytes: tag and inner length |
| CheckoutOk resource payload | Complete resource plus 6 bytes: tag, inner length, writable flag |

The bulk frame bound is complete-resource size plus six envelope bytes, separate from its outer four-byte length prefix.
The receiver checks that outer bound before frame allocation, then the inner resource limit before copying the blob.
Other controls do not inherit the bulk allowance.
An over-1-MiB libsecret section can pass daemon open under the resource cap yet fail generation-zero readiness.
A publication within its own cap must also compose with Checkout env/files within the complete-resource cap.
Oversized post-run complete state suppresses both terminal verbs and emits a safe diagnostic without replacing valid target status.

## Environment and files

The target uses `env_clear()` followed by the fixed built-in passthrough set, configured additional passthrough, trusted fixed `envSet`, and projected resource env, in that precedence for ordinary names.
The built-in set cannot be subtracted per wrapper and includes terminal/locale variables plus TMPDIR, SSH agent/display sockets, and proxy variables.
Reachability still depends on DAC and namespaces, but reachable paths/proxies can steer trusted-target behavior.
`PATH` is not inherited by default and dynamic-linker steering is rejected by configuration.

`HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_STATE_HOME` cannot be resource env names.
Name validation happens before projected-value interpretation; projected values must be valid UTF-8 and contain no NUL.
The fully composed environment is checked for keys containing `=`/NUL and values containing NUL.
Arbitrary bytes remain valid backend storage even when they are not exportable as environment strings.
Env-disabled wrappers skip env projection/name validation but not complete typed parsing.

HOME defaults to the caller's passwd home path, which grants no access by itself to the dedicated UID.
Config/data/cache/state defaults derive from effective HOME; `XDG_RUNTIME_DIR` is an independent target-owned `0700` directory and cannot be overridden per wrapper.
The caller's logind runtime tree is not automatically mounted.
[Configuration](configuration.md) owns the exact variable and path rules; [Operations](operations.md) owns known-good recovery for outside-contract forced env entries that ordinary admin deletion cannot repair.

Files planning validates all configured clean relative mounts and all file keys before effects.
Each key must route to exactly one strict mount prefix, must not equal a mount path, and must not collide as ancestor/descendant of another regular-file key.
Realization consumes only the complete plan and retained projection directory handles, stages regular files on the private capped `noswap`, mode-`0700` tmpfs, and binds clones of the held directory objects into the caller-home view with `open_tree` and `move_mount`.
Materialization and collection resolve each child relative to retained directory descriptors with `O_NOFOLLOW`; ancestor or staging-name replacement cannot redirect them.
The host retains the original projection objects until authorized collection completes, independently of the target's bind-mount names.
Collection uses fresh directory streams, accepts only regular files and directories, and rejects symlinks and unsupported entries.
Target-home attachment points are opened component-wise and bound through held destination descriptors; caller changes to their names can still affect target path reachability, not the host's staging authority.
The target sees writable staging regardless of the persistence flag.

`accessPaths` is a separate explicit host-access mechanism using one caller-to-target UID and GID idmap.
Target writes on those mounts are caller-owned in the host filesystem; other owners remain unmapped.
The named directory's filesystem must support idmapped mounts.
Nested submounts are processed independently; unsupported nested filesystems are skipped with a warning rather than silently claiming mapped access.
Paths are lexical configuration contracts, not normalized or resolved equivalence classes; see [Configuration](configuration.md).

## Secret Service publication

Libsecret uses three separate channels: fd 20 between host and daemon, a one-shot initialization pipe from PID 2 setup to secretsd, and an invocation-private CLOEXEC Unix stream publication socketpair.
The host alone owns the publication receiver; secretsd alone owns the long-lived publisher and never receives fd 20.
There is no target-owned final-state pathname.

The namespace branch closes its receiver, the host closes its publisher, PID 1 drops its publisher copy after forking PID 2, and PID 2 drops its copy after forking secretsd and before readiness or target exec.
The dbus-daemon child explicitly closes publication state.
Post-fork helpers consume every unneeded owned endpoint before setup: dbus-daemon retains only its address writer, and secretsd retains only init reader, readiness writer, and publisher among helper endpoints.
Only selected exec endpoints have CLOEXEC cleared; secretsd immediately restores publication CLOEXEC after adoption and becomes non-dumpable before reading initialization secrets.
The helper's gate writer stays CLOEXEC so successful helper exec closes its copy.
Controlled errors and caught panics terminate via `_exit` without unwinding inherited PID-2 owners; this reviewed ownership boundary is not a generic async-signal-safety guarantee.

Private helpers receive cleared environments with a trusted allowlist, not caller-selected DBUS/XDG/linker steering.
D-Bus readiness is synchronized by `--print-address=<fd>` and a pipe, not by socket-path appearance.
Secretsd initializes once, publishes generation zero, observes its complete matching ACK, and only then reports readiness that permits target exec.
The private bus tmpfs contains the filesystem socket and helper artifacts, not brokered state files, and makes no `noswap` claim.
Mount isolation does not hide abstract Unix sockets; a nonconforming client ignoring `DBUS_SESSION_BUS_ADDRESS` could attempt a reachable abstract host bus.

### Framing and state validation

Publication requests contain fixed magic/version, PUBLISH tag, big-endian `u64` attempt generation, big-endian `u32` body length, and exactly that body.
Responses carry fixed magic/version, ACK or REJECT, and the matching generation.
Generation zero is mandatory and every sent attempt consumes the next generation.
Replay, gaps, overflow, malformed framing, over-cap declarations, and transport truncation poison the stream rather than resynchronize.
The body bound is checked before allocation.

Every selectable body must parse as `LibsecretSection`, equal its JCS re-encoding byte-for-byte, and satisfy:

- Collection ids and alias names: 1–128 bytes of ASCII letters, digits, or underscore, including leading digit/underscore.
- Item ids: exactly 32 lowercase hexadecimal characters.
- Alias targets: valid existing collection ids, with exact derived one-segment collection/item/alias path structure.
- All four collection/item created/modified fields: unsigned safe integers at most `2^53 - 1`.
- Composition with the immutable Checkout env/files baseline: within the complete-resource cap.

Labels, content types, attribute keys/values, and secret bytes are not narrowed by those graph checks.
Safe timestamps need not be plausible, ordered, or monotonic.

### Candidate commit and terminal selection

A mutation holds the store write lock while cloning accepted state exactly once, preparing the candidate, canonicalizing, publishing, awaiting its exact matching ACK, and committing it to live memory.
Preparation, canonicalization, rejection, EOF, response mismatch, and transport failure leave the accepted store unchanged.
Owned discarded candidate secret vectors are wiped, as are displaced/deleted/replaced vectors from the former store after commit.
This is an owned-buffer policy, not comprehensive erasure.

The host retains a validated candidate before responding and selects it as latest only when its entire ACK has been written.
Secretsd commits only after observing that ACK; the two observations are deliberately distinct.
After gate observation, the host drains publication while polling relay exit.
Following clean reap, terminal EOF permits the latest ACK-written validated generation, including one secretsd or the D-Bus caller never observed as successful.
Later rejected, partial, response-failed, or publisher-death attempts preserve that previous generation after EOF.
Final nonblocking `EAGAIN` instead proves endpoint retention and suppresses all collection and both terminal verbs without generation fallback.
ACK means invocation-local host retention, not D-Bus response delivery, backend persistence, or external custody.

## Secret Service input

`underwrap-secretsd` implements `org.freedesktop.secrets` for the trusted target, but each handler input is robustness-untrusted.
The in-memory `underwrap-secretsd-store::SecretStore` owns the data model, not JSON, filesystems, sockets, publication, or erasure policy.
Typed-handler tests and publication validators do not establish complete Secret Service draft or D-Bus wire conformance.

Collection creation and alias operations share the segment rule above, and object-path construction is fallible.
`SetAlias` validates the alias first, then accepts only `/` to clear or the exact `/org/freedesktop/secrets/collection/<one-valid-segment>` shape to set.
Malformed alias/path returns value-free InvalidArgs before locking or publication.
A set operation checks collection existence under the write lock before candidate publication, so check and mutation see the same accepted state.
Root clear, missing-alias clear, and setting the existing target remain valid idempotent mutations through publication and ACK.

`Collection.CreateItem` defaults absent `org.freedesktop.Secret.Item.Label` to empty string and absent `org.freedesktop.Secret.Item.Attributes` to empty map.
Present properties require exact outer D-Bus types `s` and `a{ss}` respectively; nested variants are not accepted as those types, and unknown properties are ignored.
Label is validated before Attributes, giving deterministic error precedence.
Wrong-type errors are fixed value-free InvalidArgs naming the property and expected type, before secret metadata tracing, locking, candidate/random-id creation, publication, or object registration.
Valid replacement uses subset attribute matching, including vacuous matching when Attributes is absent or empty.
Item identifiers are generated from 16 random bytes encoded as lowercase hex.

Successful Item.SetSecret, Item.Attributes, and Item.Label mutations sample one Unix second and set only the item's and containing collection's modified fields to that same value.
That second can equal the previous value and has no monotonic guarantee.
These timestamps enter accepted live state only with the candidate's matching ACK; failed mutations leave the baseline for the next candidate unchanged.
Target-induced indefinite D-Bus stalls remain outside availability guarantees.

## Post-run dispatch

Disabled files/libsecret projections preserve their original sections; env remains the original backend-owned section.
After authorized collection, the host canonicalizes the composed complete resource and compares it with original Checkout bytes.
Unchanged state sends UsageSignal.
Changed state with a read-only Checkout hint is discarded and also sends UsageSignal; changed writable state attempts Checkin.
An Unauthorized Checkin response triggers a subsequent UsageSignal attempt, which can itself fail current backend policy.
Other Checkin errors remain diagnostic rather than an automatic writeback retry.
The daemon's sequential request loop is not a single-use channel state machine.

An oversized complete state returns before either terminal verb.
Rejected writeback emits `writeback_rejected` with resource identifiers and `permission_readonly`, `server_rejected`, or `resource_too_large`; size rejection adds attempted/public-limit lengths.
The optional `UNDERWRAP_SUPPRESS_READONLY_WARNING=1` is consumed by the wrapper, never passed to the target, and does not suppress audit or oversized-resource stderr.
[Operations](operations.md) owns event fields and diagnosis; [Backend](backend.md#finalize-and-concurrency) owns reauthorization and last-writer-wins persistence.

## Identity installation

The frontend runtime identity is one private PEM containing its certificate chain followed by matching private key, named by `backend.client_identity_file`.
NixOS uses `/var/lib/underwrap-<instance>/client-<instance>.identity.pem` inside the instance-owned `0700` state directory.
Keygen resolves it from `--instance-config` or uses the explicit `--client-identity-out` path; it does not enroll with the backend.

Config-derived daemon UID/GID lookup uses reentrant NSS output/buffers through validated copy, the same 16-KiB fallback and checked ERANGE growth to 1 MiB described for daemon group lookup.
Errors abort before generation/mutation; only ordinary not-found admits the no-local-daemon fallback.
Resolved config-derived daemon ownership takes precedence, otherwise existing identity ownership is preserved, otherwise the invoking identity is used.

Keygen prepares one same-parent mode-`0400` temporary file, writes the complete PEM, applies exact owner/mode, syncs it, and commits through one `NamedTempFile::persist()` replacement.
Precommit failure preserves old complete bytes/owner/mode and removes the temporary file.
After that visibility commit, public-certificate stdout can still fail while the new identity remains installed.
There is no transaction between persistence and stdout, or installation and enrollment, and no parent-directory fsync, power-loss durability, or media-erasure promise.
Candidate identity buffers are wiped where practical.

Daemon startup rejects a present identity owned by another UID or wider than `0600`.
When parent metadata is obtainable, it also rejects another owner or a mode wider than `0700`; metadata lookup failure skips that parent check.
Missing identity is allowed for initial enrollment and wrapper requests retry client construction until successful.
Once built, the combined identity/client is cached; replacement requires explicit restart, not live reload.
The sandboxed daemon has read-only state/config access, runtime-directory write access, no capabilities or new privileges, and restricted system/home/device/namespace/syscall access.
It still needs cross-user `/proc/<pid>/status` visibility for peer authentication.
Enrollment, replacement ordering, interruption, and service ownership checks belong in [Operations](operations.md).
