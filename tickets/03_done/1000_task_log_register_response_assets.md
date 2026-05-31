# Task: Log the named assets from the runtime register response

## Problem

`POST /v1/wrappers` returns a fully structured payload naming exactly
what the call created or would create:

- `RuntimeWrapperRegisterResponse::Ok { auto_created_resources,
  auto_created_pairs }`
- `RuntimeWrapperRegisterResponse::AutoCreateDisabled {
  missing_resources, missing_pairs }`

The backend builds these lists honestly, but the only consumer — the
daemon's startup register step — collapses **both arms to `.len()`**
and discards every name:

`crates/frontend-underwrapd/src/server.rs:116-140`

```rust
info!(
    created_resources = auto_created_resources.len(),
    created_pairs = auto_created_pairs.len(),
    "backend auto-created wrapper assets on register",
);
...
warn!(
    missing_resources = missing_resources.len(),
    missing_pairs = missing_pairs.len(),
    "backend refused wrapper register: auto-create disabled ...",
);
```

So we serialize `Vec<String>` / `Vec<RuntimeWrapperRegisterPair>` over
mTLS that nobody reads, and the `AutoCreateDisabled` warning tells the
operator to go look at the admin UI rather than naming the missing
assets it is literally holding. Worst of both worlds: the detail exists
but doesn't earn its keep.

## Decision

Keep the structured response (it is genuinely useful) and **make the
daemon log the names**, so the payload earns its place and an operator
diagnosing a failed/at-startup register sees exactly which resources and
pairs were created or are missing — without a second trip to the admin
UI.

## What to do

1. In `register_wrapper_assets`
   (`crates/frontend-underwrapd/src/server.rs`), log the actual names
   instead of counts:
   - `Ok` arm: emit the created resource names and the created
     `(wrapper, resource)` pairs. Keep the no-op fast path quiet (only
     log when something was created).
   - `AutoCreateDisabled` arm: name the missing resources and missing
     pairs in the warning, in addition to (or instead of) the
     "enable auto_create / create manually" guidance.
   Format the pair lists readably (e.g. `wrapper=>resource`), and keep
   it to one structured event per arm — do not spam one line per asset.
2. Confirm no other consumer relies on the counts-only shape.
3. Do **not** change the wire types — this is a logging-only change.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- A manual or test check that the register log lines now carry the
  resource/pair names (the daemon register tests in
  `frontend-underwrapd` are the natural place to assert the names reach
  the log, if a test seam exists).
