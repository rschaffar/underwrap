---
name: reviewing-code
description: Perform an ordinary bounded read-only review of Underwrap diffs, commits, pull requests, or source changes.
compatibility: Intended for the Underwrap repository and its documented security and validation boundaries.
---

# Underwrap reviewing code

Review the requested diff, commit, pull request, or bounded source change rather than silently expanding to a whole-repository audit.

This skill is distinct from `underwrap-codebase-review`.

Use this skill for ordinary bounded review.

Use `underwrap-codebase-review` for a whole-repository review or an explicitly comprehensive subsystem review.

Treat the review as read-only unless the user explicitly requests a separately authorized fix.

Do not silently materialize a plan, create a ticket, or implement a fix from a review finding.

## Workflow

1. Determine the exact requested paths, commit range, pull request base, and current working-tree state.
2. For branch or pull-request work, use the actual merge base and do not assume a branch name.
3. Read `AGENTS.md`, `docs/plans/README.md`, the active plan that owns the change, relevant ADRs, `docs/architecture.md`, `docs/security.md`, `docs/testing.md`, and any historical review record that the plan cites.
4. If an existing legacy ticket is referenced, read it as historical scope and preserve its recorded constraints without creating a new ticket.
5. Infer intended behavior from the request, owning plan, current documentation, tests, configuration, and surrounding code.
6. Trace changed behavior through callers, wire representations, state transitions, errors, cancellation, cleanup, persistence, and downstream consumers.
7. Check only security and integration domains reachable from the requested change.
8. Run focused checks when feasible and distinguish command success from proof of a broader claim.
9. Report confirmed findings first, then inferred risks and unverified areas.
10. Stop after the requested change, affected contracts and callers, and focused evidence are covered.

## Underwrap security checks

Name the protected asset, attacker, boundary direction, intended invariant, and evidence for every security finding.

### Caller and target boundary

Confirm that the caller remains untrusted with respect to the brokered secret, including other caller-owned non-root processes.

Confirm that the configured target is trusted with the projection but target-supplied protocol input remains untrusted for robustness and storage integrity.

Confirm that the review does not accidentally turn `allowedUsers` PATH shims into an authorization claim or treat a local Unix user as a backend principal.

### Runtime identity and authorization

Confirm that runtime client identity comes from the authenticated mTLS certificate fingerprint and the backend-selected store.

Confirm that wrapper names remain selectors rather than authorization, unknown names and the reserved `debug` selector are rejected, and `argv[0]` is not trusted.

Confirm that the backend remains policy authority and that runtime requests cannot name a storage key or `control/*` document.

Confirm that permission mode is re-evaluated on open and finalize and that writable state is re-checked on every submitted writeback.

### Local peer authentication

Confirm the effective-GID instance socket gate and its failure behavior.

In the hardened posture, confirm that `SO_PEERPIDFD` is the selected path and that acquisition, resolution, status parsing, and credential mismatches reject without falling back.

Confirm that the `SO_PEERCRED` plus `/proc` path is used only for the explicitly weaker non-hardened posture.

### Privileged handoff and target identity

Confirm that fixed fd 20 is an authenticated `AF_UNIX` stream endpoint with the root-provisioned daemon identity before fd 3, selectors, target arguments, or privileged setup are trusted.

Confirm that fd 3 carries descriptor-free control data and cannot select an alternate resource channel.

Confirm that target UID/GID authority comes only from the root-owned `(instance, wrapper)` table.

Confirm permanent real, effective, and saved ID changes, supplementary-group cleanup, `PR_SET_NO_NEW_PRIVS`, capability clearing, and verification of the final identity.

Confirm that target and helper processes do not inherit fd 20, fd 3, lifecycle descriptors, publication endpoints, or other descriptors above stderr.

Confirm environment clearing, the documented allowlist, forced values, `LD_*` rejection, core limits, dumpability order, and secret-buffer cleanup.

### Namespaces and projections

Confirm fresh PID, mount, IPC, and UTS namespaces where the changed flow requires them.

Confirm private mount propagation, `noswap` tmpfs staging, idmapped caller-directory access, target-owned runtime paths, and namespace teardown behavior.

Confirm that files and libsecret projections remain locally writable even when backend persistence is read-only, and that discarded changes are not claimed to be a mount-level read-only behavior.

Confirm libsecret publication ownership, readiness synchronization, canonical validation, generation and ACK rules, untrusted D-Bus input handling, and target/helper endpoint closure.

### Persistence, storage, and writeback

Confirm that brokered secret persistence uses `StorageBackend`, apart from the documented namespace-private tmpfs staging exception.

Confirm atomic replacement and error ordering for storage, registry, resource, Secret Service, and credential operations touched by the change.

Confirm that the review does not claim compare-and-swap or transaction semantics where the current whole-blob load/compare/save behavior remains last-writer-wins.

Confirm that failed pre-exec setup, cancellation, partial publication, response loss, and finalization errors preserve the documented state and target-status behavior.

### HTTP, admin, and SPA contracts

Check HTTPS-only runtime transport, selected CA trust, mTLS handling, base paths, redirects, request and response bounds, error envelopes, and generated OpenAPI meaning.

Check admin authentication, session invalidation, CSRF, origin checks, login/bootstrap boundaries, rate limits, and error disclosure when reachable.

Check SPA routing, generated API types, UTF-8 and base64 byte handling, form validation, stale-state behavior, accessibility impact, and browser-test alignment when reachable.

### NixOS and release integration

Check NixOS assertions, users, groups, capabilities, services, wrappers, generated configuration, path and name grammar, architecture selection, and package references.

Require the NixOS VM test for changes affecting the module, privileged wrapper chain, target identities, capabilities, namespaces, mounts, peer authentication, or deployed service topology.

Check Rust, web, OpenAPI, and Nix integration rather than relying on one layer's unit tests.

## General correctness checks

Check malformed frames, wrong types, control characters, size limits, partial reads and writes, retained endpoints, response loss, retries, and error precedence.

Check state captured before an await, concurrent requests, cancellation, timeout cleanup, child lifetime, lock scope, retry behavior, and partial initialization.

Check secret, token, key, and resource lifetime through serialization, errors, tracing, process environments, temporary paths, logs, and test diagnostics.

Inspect changed unsafe code for pointer validity, alignment, lifetime, ownership, fd ownership, syscall return values, and a nearby invariant explanation.

Check that tests fail for the claimed pre-fix reason, cover both permitted and denied paths where relevant, and do not overstate simulated or fixture evidence.

Check that new or changed documentation uses current Underwrap vocabulary and does not turn internal evidence into a public security guarantee.

## Findings

Lead with confirmed findings ordered by severity.

Use `Critical` for a direct security-claim breach, likely secret exposure, destructive integrity failure, privilege or identity bypass, or inability to operate the supported product.

Use `Major` for material security, correctness, availability, compatibility, or maintainability risk that should block acceptance.

Use `Minor` for a real bounded defect with low impact.

Each finding includes:

- a concise title and severity;
- the concrete trigger or input;
- the affected path and line when available;
- the impact and protected asset;
- the attacker or failure source and boundary direction when security-relevant;
- implementation, test, or documentation evidence;
- the smallest actionable correction;
- related plan tasks or historical findings without duplicating their ownership;
- checks run and checks that remain unavailable.

Do not report preference-only style comments or speculative redesigns.

If no finding survives verification, say so and name meaningful residual risks or unavailable checks.

Give a merge verdict only when the user requests mergeability or a pull-request decision.
