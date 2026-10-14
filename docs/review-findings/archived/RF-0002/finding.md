# RF-0002 — Dynamic-debug SPA fallback can disclose an outside symlink target

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

An admin-enabled backend exposes its SPA fallback outside the admin authentication gates.
In `crates/underwrap-backend/src/web.rs:57-72`, `serve_spa` removes leading slashes but passes parent components to `EmbeddedAssets::get`.
The routing is at `crates/underwrap-backend/src/http/mod.rs:63-89`.

With debug assertions enabled and without rust-embed's `debug-embed` feature, lookup is dynamic filesystem access, not the table lookup described by the source comment.
The locked rust-embed implementation permits an outside canonical target when the requested final component is a symlink.
Even a symlink outside the asset root and absent from the asset inventory can satisfy this exception.
Traversal to an ordinary external regular file does not.

The trigger requires a reachable dynamic-debug service, enabled administration, a request retaining its parent components, a suitable known/requestable final-component symlink, and a target readable by the backend process.
No admin session, runtime certificate or attacker-controlled local write is needed if the symlink already exists.
A custom profile must be assessed by its actual debug-assertion/embedding behavior, not its name.

## Consequence and boundary

The boundary is backend filesystem content to a public, unauthenticated response, contradicting the nonsecret-bundle boundary at `crates/underwrap-backend/src/web.rs:3-4,21-23`.
Confidentiality impact can be High if a sensitive target is available; the witness establishes neither that availability nor deployment likelihood.
Ordinary packaged release embedding performs exact asset-table lookup and is explicitly excluded.
This is not an audit of content intentionally embedded by a trusted build.

The original suggestions involving descriptor or special-filesystem symlinks remain unverified leads, not demonstrated exploit paths.
No live credential, private resource or host symlink inventory was inspected.

## Correction and alternatives

Constrain public retrieval to the intended asset set in every network-serving build, including dynamic debug, or deliberately make embedding-only behavior a network-serving prerequisite.
Retain normal asset MIME/cache handling, public shell delivery, history fallback and HEAD semantics.
Restricting debug artifacts to local development is an operational alternative, not evidence that existing exposed debug deployments are safe.
The completed correction selects embedding-only behavior, as recorded below.

## Acceptance criteria

- Exercise the real handler/router with an outside final-component symlink absent from the inventory; it must not return the outside bytes.
- Retain ordinary external-file traversal as a separate negative control and valid asset/index/history requests as positive controls.
- Check response bodies, not status alone: fallback legitimately returns index content with status 200.
  HEAD's empty body alone cannot establish confinement.
- Retain release-handler and packaged-release negative controls; check actual build features/profiles.
- If deployment-level closure is claimed, run a disposable full debug HTTPS case with authentication absent and confirm the same confinement.
  The fixed-build checks are recorded in the resolution evidence.

## Work ownership

This record owns the authorized, bounded correction: embed the SPA in every backend build profile.
The protected asset is backend-readable content outside the nonsecret bundle; the attacker is an unauthenticated requester crossing from HTTP into filesystem reads.
The invariant is that public website requests resolve only compiled asset keys.
No dependency addition, runtime/admin API redesign, local privileged-chain change or deployment change is in scope.

- [x] Enable `debug-embed` and document the backend rebuild requirement for development.
- [x] Reproduce the outside final-component symlink failure before the fix, then pass the real-handler/router regression in debug and release with shell, history, regular-file traversal, asset and HEAD controls.
- [x] Run backend tests, Rustfmt, denied-warning Clippy and relevant contract/SPA checks; obtain independent review and record the actual evidence and limits.

Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
