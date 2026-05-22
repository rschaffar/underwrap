# Task: Repo sweep — drop ticket references in code and docs

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-014, F-015 (review report `web_spa_2026-05-22.md`)

## Background

Per the project posture in `CLAUDE.md` and the
`underwrap-component-refine` skill: **no ticket / epic / story
references in code, config, or docs.** Tickets and commit
messages are the right place for that context.

Two known hits in this review:

- `web/src/api/client.ts:372` — `// ----- Client instances admin (story 340) -----`
- `docs/backend.md:491` — `…will be a byte comparison once the
  wrapper-side rewrite (story 410)`

Project-wide sweep ticket: fix the two known hits, then run the
verification grep below to catch any regressions introduced
between the review and now (or in parallel tickets).

## Scope

### Known hits

- `web/src/api/client.ts:372` — drop "(story 340)" so the
  divider reads `// ----- Client instances admin -----`.

- `docs/backend.md:491` — drop "(story 410)" and rewrite the
  sentence to describe the current/intended state without
  naming a ticket. Something along the lines of:

  > will be a byte comparison once the wrapper-side rewrite
  > lands.

  …or, if the wrapper-side rewrite has since landed, update
  the sentence to past tense and describe what's true now.

### Verification grep

After the edits, both of the following must return nothing
(excluding the `tickets/` tree itself, which legitimately
contains the references):

```
rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' web/src/ docs/ examples/ crates/ nix/
rg -n -i '(story \d+|stories \d+)' web/src/ docs/ examples/ crates/ nix/
```

If the second grep surfaces any prose mentions of `story NNN`
that don't match the first grep's stricter patterns, treat
them the same way: rewrite without the ticket pointer.

## Acceptance criteria

- [ ] `rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' web/src/ docs/ examples/ crates/ nix/`
      returns nothing.
- [ ] `rg -n -i '(story \d+|stories \d+)' web/src/ docs/ examples/ crates/ nix/`
      returns nothing.
- [ ] `cargo test --workspace` passes (text-only edits, but the
      `docs/` directory may be referenced by doctests — verify).
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
