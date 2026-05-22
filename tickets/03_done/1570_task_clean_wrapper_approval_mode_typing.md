# Task: Clean up `WrapperApprovalMode` typing — drop SPA casts

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-016, F-026 (review report `web_spa_2026-05-22.md`)

## Background

The SPA constructs and destructures `WrapperApprovalMode` in 11
places. Every one carries an `as WrapperApprovalMode` or
`as unknown as WrapperApprovalMode` cast, plus a
`(mode as { until_epoch: bigint | number }).until_epoch`
field-access cast at two sites.

Production sites:
- `web/src/pages/wrapper-permissions.tsx:58` — `modeKind` returns
  `mode.kind as ModeKind`.
- `web/src/pages/wrapper-permissions.tsx:590` —
  `<ModeBadge mode={{ kind: "locked" } as WrapperApprovalMode} />`.
- `web/src/pages/wrapper-permissions.tsx:616` —
  `Number((mode as { until_epoch: bigint | number }).until_epoch)`.
- `web/src/pages/wrapper-permissions.tsx:938` — same field-access
  cast.
- `web/src/pages/wrapper-permissions.tsx:987, 990, 1001` — the
  `switch (kind)` block constructs each variant with `as
  WrapperApprovalMode` (and `as unknown as WrapperApprovalMode`
  for `unlocked-until` because of the `bigint` vs `number`
  mismatch).

Test sites:
- `web/src/pages/wrapper-permissions.test.tsx:27, 35, 118-126,
  134-143` — five more casts of the same shape.

Two root causes:

1. **Object-literal widening.** `{ kind: "locked" }` is inferred
   as `{ kind: string }`, not `{ kind: "locked" }`, so it doesn't
   fit the discriminated union without help. The current SPA
   reaches for `as WrapperApprovalMode`; the right tools are
   `as const` or an explicit variable annotation
   (`const mode: WrapperApprovalMode = …`).

2. **`bigint` vs `number` for `until_epoch`.** The Rust field is
   `u64`, which ts-rs renders as `bigint`. The SPA passes
   `number` (`Math.floor(until / 1000)`) and reads it back as a
   `number` (JSON has no BigInt — fetch's `res.json()` returns a
   `number`). The mismatch lives only in the TypeScript type;
   the wire shape is identical. Fixable on the Rust side with
   `#[ts(type = "number")]`.

The generated TS already IS a discriminated union:

```ts
export type WrapperApprovalMode =
  | { "kind": "locked" }
  | { "kind": "unlocked" }
  | { "kind": "unlocked-until", until_epoch: bigint, };
```

So the structural shape is fine; the friction is the `bigint`
field type plus the SPA's habit of casting literals.

## Scope

### Rust side

- `crates/shared-protocol/src/policy.rs:39-48` — add
  `#[ts(type = "number")]` to `until_epoch`:

  ```rust
  pub enum WrapperApprovalMode {
      Locked,
      #[default]
      Unlocked,
      UnlockedUntil {
          #[ts(type = "number")]
          until_epoch: u64,
      },
  }
  ```

  No wire change: `u64` still serializes as a JSON number.
  Only the generated TypeScript flips from `bigint` to `number`.

- Run `cargo test --workspace`; the ts-rs `ts_export` test
  regenerates `web/src/api/types/WrapperApprovalMode.ts` and
  fails until the file is regenerated. If the golden file is
  checked in, update it from the test output (the test prints
  a unified diff — copy the new content).

### SPA side

Drop every cast. Suggested patterns:

- `wrapper-permissions.tsx:58` — `modeKind` returns
  `mode.kind` directly (the union type already gives `"locked" |
  "unlocked" | "unlocked-until"` for `kind`).

- `wrapper-permissions.tsx:590` —
  `<ModeBadge mode={{ kind: "locked" }} />`. The prop type is
  `WrapperApprovalMode`; TS narrows the literal against the
  expected type at the call site, so no cast is needed.

- `wrapper-permissions.tsx:616, 938` — replace the
  `(mode as { until_epoch: ... }).until_epoch` access with proper
  narrowing:

  ```ts
  if (mode.kind === "unlocked-until") {
    const until = mode.until_epoch;
    // …
  }
  ```

- `wrapper-permissions.tsx:982-1004` — the `switch (kind)` block
  constructs a `WrapperApprovalMode` value. With an explicit
  variable annotation TS narrows each arm:

  ```ts
  let mode: WrapperApprovalMode;
  switch (kind) {
    case "locked":
      mode = { kind: "locked" };
      break;
    case "unlocked":
      mode = { kind: "unlocked" };
      break;
    case "unlocked-until": {
      const until = new Date(untilLocal).getTime();
      if (Number.isNaN(until)) { setSubmitError("…"); return; }
      mode = { kind: "unlocked-until", until_epoch: Math.floor(until / 1000) };
      break;
    }
  }
  ```

- `wrapper-permissions.test.tsx:27, 35, 118-126, 134-143` — the
  test fixtures construct mode literals the same way. Replace
  `{ kind: "locked" } as WrapperApprovalMode` with `{ kind:
  "locked" } as const` (or annotate the surrounding
  `WrapperPermission` so narrowing flows down). Replace the
  `until_epoch: 1n` literal at line 125 with `until_epoch: 1`
  (number) — the type now expects `number`.

### Decision boundary

Stay narrow: only `until_epoch` flips to `#[ts(type = "number")]`
in this ticket. Other u64 timestamp fields (`created_at`,
`last_used_at`, `password_changed_at`) keep their existing
`bigint` rendering. The pages already cope via the
`normalizeEpoch` helper (introduced in 1510). If a wider
"render u64 as number" decision is wanted later, that's a
separate ticket.

## Acceptance criteria

- [ ] `rg -n 'as WrapperApprovalMode' web/src/` returns nothing.
- [ ] `rg -n 'as unknown as WrapperApprovalMode' web/src/`
      returns nothing.
- [ ] `rg -n '\(mode as \{ until_epoch' web/src/` returns nothing.
- [ ] `rg -n 'as ModeKind' web/src/` returns nothing.
- [ ] `web/src/api/types/WrapperApprovalMode.ts` shows
      `until_epoch: number` (not `bigint`).
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (wrapper-permissions
      e2e exercises the `unlocked-until` flow).
