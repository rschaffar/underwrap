# Task: Fix the "debug tool is read-only / prints secrets" framing

## Problem

The debug tool and the `Debug` variant are described across docs and
comments as a read-only *inspection* path that *prints* the projected
secret. That is wrong and understates its capability.

`crates/debug-tool/src/main.rs` exposes three subcommands:

- `Summary` (default) — prints an environment summary.
- `Shell { args }` — execs an **interactive shell** inside the wrapped
  namespace (`exec_shell` → `exec_command`).
- `Exec { command, args }` — execs an **arbitrary command** inside the
  wrapped namespace.

So a `Debug` invocation is a full interactive session with **read-write**
access to the projected secret: env vars set, files bind-mounted, the
private libsecret bus live. Anything that process writes to the staged
files / libsecret state is picked up by `frontend-ns`'s variant-blind
`dispatch_writeback` and, **if the permission is `writable`**, checked
back into the store via a `Checkin` carrying `variant=Debug`. The
backend accepts it exactly as it would a `Real` writeback
(`runtime_api/invocations.rs` finalize: `decide(UnsafeDebugUntil,
Debug)=Allow`, then the `writable` re-check).

This is **intended behavior** — a writable debug grant that can repair a
secret interactively is legitimate debugging. The bug is purely the
*framing*: nothing should describe debug as read-only / print-only /
inspection-only.

## Corrected framing to apply

The `Debug` variant runs `underwrap-debug-tool`, which can print a
summary **and** drop an interactive shell or exec an arbitrary command
inside the wrapped namespace with **full read-write** access to the
projected secret. Writebacks from a `Debug` run are accepted on the same
terms as `Real` — gated only by the permission's `writable` flag.
`Debug` is not read-only; `UnsafeDebugUntil` controls *whether the debug
variant may run at all*, not whether it may write.

## Audit + sites to fix

Re-run the audit (don't rely only on the list below — phrasing varies):

```
rg -rn -i "inspection|prints|projected secret material|read-only" \
  --glob '*.rs' --glob '*.md' crates/ docs/ CLAUDE.md examples/ \
  | rg -i "debug|unsafe"
```

Known sites:

1. `CLAUDE.md:64` — "Unsafe inspection CLI" in the project structure
   list. Rephrase (e.g. "Unsafe debug shell/exec tool …").
2. `CLAUDE.md:320-322` — "the debug tool, which prints projected secret
   material". Replace with the corrected framing; keep the
   `not-unsafe-debug` denial wording.
3. `docs/backend.md:481-482` — "The only mode under which the debug tool
   runs; it prints the projected secret material …". Fix.
4. `docs/architecture.md` — the `variant = Debug (prints projected
   secret material)` annotation (diagram/legend around line 234 and the
   variant description). Fix.
5. `docs/wrapper-author-checklist.md:169-170` — "if an operator needs to
   inspect the projected secret … `UnsafeDebugUntil` enables the debug
   tool". Note it is a full RW shell/exec, not inspection, and that a
   writable grant lets the session write the secret back.
6. `crates/debug-tool/src/main.rs:9` — `about = "Unsafe developer
   inspection tool for wrapped environments"`. Broaden to reflect
   shell/exec + RW (e.g. "Unsafe developer shell/exec tool …").
7. `docs/security.md` — scan the debug-tool / `UnsafeDebugUntil`
   paragraphs (around lines 200-217, 348-373) and ensure none imply
   read-only debug. Add one sentence making the RW-writeback capability
   explicit where `UnsafeDebugUntil` is introduced.

While auditing, also fix any *code comments* (not just the runtime
service types) that call the debug path inspection/read-only. The
runtime `service.rs` Debug-related comments and `variant.rs` should be
checked for the same overstatement.

## Constraints

- **Docs/comments only. No behavior change.** Do not add a
  debug-writeback denial — the decision is that writable debug grants
  legitimately write back.

## Acceptance

- The audit grep above returns no remaining read-only/print-only/
  inspection-only framing of the debug tool or `Debug` variant.
- `cargo fmt -- --check`
- `cargo build` (the `about` string change compiles).
