# Underwrap edge-case checklist

Use this as a bounded prompt library rather than a requirement to test every item.

Select only domains reachable from the requested flow.

## Protocol sizes and malformed frames

- Empty, maximum-size, oversized, truncated, duplicated, and unexpected resource or control frames.
- Byte limits versus character counts for UTF-8, base64, canonical blobs, publication bodies, and HTTP envelopes.
- Missing, extra, partial, or multiple messages, invalid JSON, wrong types, unknown fields, trailing data, and control characters.
- Reserved names, delimiter ambiguity, path separators, invalid identifiers, and equivalent normalized forms.
- Slow readers or writers, disconnected peers, response loss, retained writers, timeout cleanup, and retry after an unknown remote outcome.
- Connection limits, queue bounds, staged-resource limits, and repeated requests that never make progress.

## Identities and authorization

- Missing, malformed, disabled, expired, duplicated, or reassigned runtime client identities.
- Certificate fingerprint, proof of possession, selected CA, hostname, HTTPS-only, redirect, base-path, and mTLS errors.
- Unknown wrapper selectors, the reserved `debug` selector, forged variants, untrusted target arguments, and `argv[0]` confusion.
- Effective versus real credentials, peer PID reuse, pidfd acquisition and resolution failures, `/proc` status parsing, and effective-GID mismatches.
- Missing or conflicting root-owned target-UID table rows, instance/wrapper collisions, caller fallback, root fallback, and daemon/target identity overlap.
- Admin authentication, bootstrap and recovery authority, session invalidation, CSRF token binding, origin checks, rate limits, and authorization error precedence.

## Privileges, namespaces, capabilities, UIDs, and file descriptors

- Checked results from supplementary-group clearing, permanent real/effective/saved ID changes, `NoNewPrivs`, capability clearing, dumpability, and core-limit setup.
- Fixed fd 20 domain/type/peer authentication before fd 3 or selector use, fixed fd 3 descriptor-free handoff, and rejection without alternate fd or token fallback.
- Descriptor ownership through `SCM_RIGHTS`, `dup2`, fork, exec, close-on-exec, `close_range`, and partial initialization.
- Target, helper, publication, liveness, status, and resource-channel endpoint retention or accidental inheritance.
- Namespace creation, mount propagation, idmapped mounts, nested submounts, private tmpfs, `/proc` remounting, PID-1 relay, PID-2 target, and teardown.
- Capability lifetime before and after projection, target-UID switching, helper startup, target execution, writeback, cancellation, and wait/reap.
- Absolute and relative paths, missing parents, wrong owners or modes, symlinks, wrong file types, path replacement, traversal, duplicate paths, and overlapping mounts.

## Concurrency, cancellation, retries, and partial failure

- State observed before an await that changes before continuation resumes.
- Concurrent opens, finalizes, registrations, admin edits, resource deletion, client remapping, rotation, and writeback.
- Explicit last-writer-wins behavior versus any claimed transaction, revision, lock, or compare-and-swap semantics.
- Cancellation before start, during authorization, after secret exposure, after staging, during target execution, during publication, and before terminal dispatch.
- Deadline expiry, local saturation, remote outcome unknown, response loss, reconnects, retries, duplicate terminal verbs, and stale permits.
- Child death, supervisor death, parent death, signal delivery, retained writers, wait/reap failures, and orphaned descendants.
- Partial initialization, failed persistence, failed rename, failed fsync, failed cleanup, restart, and recovery after every visible side effect.

## Secrets, zeroization, persistence, and logs

- Empty, maximum-size, malformed, partially valid, invalid UTF-8, NUL-containing, and arbitrary binary secret values.
- Copies through decoded blobs, serde values, base64, HTTP bodies, environment construction, D-Bus variants, errors, callbacks, tracing, and test diagnostics.
- Cleanup after success, rejection, serialization failure, publication failure, response loss, cancellation, timeout, panic, process death, and early return.
- Brokered-resource storage through `StorageBackend`, documented namespace-private tmpfs staging, encrypted-store key lifetime, credential-file permissions, and atomic replacement.
- Resource names, identifiers, filenames, paths, timestamps, aliases, and diagnostic samples that may contain operator-sensitive input.
- Plaintext, encoded, canonical, or length-bearing secret disclosure in service logs, stderr, journals, browser consoles, crash output, and command lines.
- Core dumps, `/proc` metadata, inherited environments, file descriptors, mount paths, and external key custody assumptions.

## Backend storage and writeback

- Complete typed resource preservation across env, files, and libsecret mutations.
- Resource, wrapper/resource pair, permission row, runtime client, store, and catalog existence and reference checks.
- Read-only local projection versus backend writable revalidation, env immutability, canonical comparison, and unchanged-state behavior.
- Active-key deletion, orphan blobs, retained encrypted generations, rotation rollback, locked recovery state, and one-time key response loss.
- Atomic save/delete visibility, live-registry publication ordering, storage errors, state re-read, and documented lack of power-loss or CAS guarantees.
- Files-only collection, symlink rejection, fsuid windows, libsecret generation selection, ACK completeness, terminal EOF, and finalization eligibility.

## HTTP, admin, sessions, CSRF, and API bounds

- Method and route coverage, request/body/header/path/query limits, malformed base64, invalid UTF-8, unknown JSON fields, and exact error precedence.
- Runtime HTTPS/mTLS and selected trust, redirect downgrade, public versus protected routes, status/code/schema accuracy, and generated OpenAPI drift.
- First-operator bootstrap, offline recovery, password hashing, session cookie attributes, auth-hash invalidation, logout success, and stale-cookie replay.
- CSRF priming, token mismatch, cookie binding, origin policy, state-changing methods, login rate limiting, and dummy-hash behavior.
- Missing stores, clients, resources, pairs, permissions, locked stores, stale URLs, list failures, and no-effect guarantees.
- Last-writer-wins, response loss, timeouts, non-transactional external effects, and safe public error messages.

## SPA and generated API contracts

- Generated schema drift, stale operation names, status/code mismatches, public-security inheritance, and request-body shape changes.
- UTF-8 text conversion, base64 round trips, empty versus missing values, invalid bytes, large inputs, and clipboard or reveal behavior.
- Form parsing, duplicates, object-like keys, line endings, validation-before-request, field errors, and error-state cleanup.
- Authentication bootstrap, protected routing, hard navigation, logout failure and retry, stale store IDs, list errors, and cache invalidation.
- Keyboard focus, visible focus indicators, forced colors, zoom, narrow viewports, and browser projects that discover no tests.
- Secret values in UI labels, errors, logs, snapshots, request previews, and browser diagnostics.

## NixOS modules, assertions, services, wrappers, and architecture checks

- Missing, wrong-typed, unknown, empty, relative, noncanonical, whitespace, newline, delimiter, traversal, duplicate, and overlapping option values.
- Final typed generator configuration versus convenience options, assertion conjunctions, negative evaluator rows, and successful/failed toplevel forcing.
- Instance and target users/groups, root-owned tables, service identities, state/runtime paths, file modes, capabilities, systemd restrictions, and wrapper modes.
- Setgid entry, capability-bearing namespace wrapper, fixed descriptor slots, daemon service readiness, socket ownership, and environment generation.
- Ordinary swap, zram swap/writeback, mounted zram filesystems, no-swap posture, kernel-version assertions, and explicitly weaker posture.
- Architecture-specific package/check outputs, source filtering, generated OpenAPI and SPA assets, examples, integration runner paths, and clean-checkout reproducibility.
- Required x86_64-linux NixOS VM evidence, aarch64 evaluation, browser and host availability, and architecture-specific checks that were skipped or unavailable.

## Test evidence quality

- The regression fails on the pre-fix tree for the claimed reason.
- Assertions capture the contract rather than an incidental implementation shape.
- Permitted and denied paths are covered for a security boundary.
- Opposite bounds, equivalent representations, alternate selectors, and sibling callers cannot bypass the fix.
- Concurrency tests control ordering rather than relying on sleeps or lucky scheduling.
- Simulated, fixture, deployed, unavailable, and user-reported evidence are labeled accurately.
- A passing unit or aggregate check is not used to claim kernel isolation, allocator zeroization, target trustworthiness, filesystem durability, or operator behavior that it does not observe.
