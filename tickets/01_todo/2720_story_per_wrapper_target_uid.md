# Story: Per-(instance, wrapper) target sandbox uids

Parent: `2600_story_target_uid_isolation` (done — this is the deferred
"finer granularity" refinement that `2600`/`2620` forward-referenced as
`2670`; `2670` was instead spent on always-isolated targets, so the
refinement never got a ticket until now).

Children:
- `2730_task_nix_per_wrapper_target_uid`
- `2740_task_runtime_isolation_key_instance_wrapper`
- `2750_task_docs_and_vm_tests_per_wrapper_uid`

## Status: TODO

## Problem

Today the dedicated sandbox uid a target runs as is keyed on the **target
binary path**: the NixOS module creates one `underwrap-target-<hash(path)>`
user per distinct absolute target path, and `underwrap-ns` looks it up in
`/etc/underwrap/target-uids` by the daemon-chosen path
(`crates/underwrap-ns/src/target_uid.rs`). The module is explicit that this
is a coarsening — *"Wrappers sharing a target binary share its uid (same
tool = same trust domain); finer granularity is future work"*
(`nix/modules/underwrap.nix:458`).

That coarsening has three consequences:

1. **Two different instances that use the same binary share a target uid.**
   Instances are advertised as "fully isolated users, groups, sockets," but
   at the target-uid layer they are not: instance A's `gh` target and
   instance B's `gh` target run as the *identical* uid.
2. **The debug tool is a single shared uid host-wide.** Every `Debug`
   invocation of every wrapper of every instance runs as
   `underwrap-target-<hash(underwrap-debug-tool)>` — and the debug tool is an
   interactive shell with read/write access to the projected secret.
3. **Two wrappers that broker *different* secrets to the same binary are not
   separated by uid.** That separation rests entirely on the Yama
   `ptrace_scope = 2` pin plus the per-invocation mount/PID namespaces. The
   threat model already warns that lowering the Yama pin "weakens the
   same-UID protection claim"; under uid sharing, the blast radius of that
   weakening spans every secret routed through the same binary.

The premise "same tool = same trust domain" is the weak point: two wrappers
pointed at the same binary almost always broker *different* secrets — that is
the reason to have two wrappers. So the binary is the wrong unit; the
**authorization unit** is `(instance, wrapper)`, which already maps to exactly
one resource/secret and is already the policy grant key
`(client_instance, wrapper, resource)`.

### Direction and asset (threat-model framing)

Against underwrap's **sole goal — hiding the brokered secret from the
same-uid caller — the current scheme is not a vulnerability**: the caller is
always uid C, the target always a dedicated system uid X ≠ C, regardless of
sharing. This story does **not** fix a caller-facing hole. It tightens
*defense-in-depth* (separation between distinct secrets / distinct instances
that currently land on one uid) and removes a real surprise (the
"fully isolated instances" wording is false at the uid layer, and the debug
tool's host-wide shared uid). It does not weaken any boundary: every uid is
still a non-root dedicated system uid ≠ caller.

## Design

**Key the uid on `(instance, wrapper)`, full stop.** A wrapper has exactly one
target binary, so `(instance, wrapper) → target path` is a function — the path
carries no information `(instance, wrapper)` does not already pin down. The new
partition is strictly finer than the path-based one.

- One `underwrap-target-<hash(instance "/" wrapper)>` system user/group per
  configured `(instance, wrapper)` pair. Name stays 29 chars
  (`underwrap-target-` + 12 hex), under the 32-char limit. Stable per pair,
  so adding/removing pairs never reshuffles existing uids (NixOS persists by
  name).
- The root-owned authority table `/etc/underwrap/target-uids` is re-keyed from
  `<path>\t<user>\t<group>` to `<instance>\t<wrapper>\t<user>\t<group>`.
  `underwrap-ns` matches on the first two columns.
- **The `(instance, wrapper)` selector reaches `underwrap-ns` from
  `underwrap-exec`, which already holds both authoritatively** — the instance
  from its effective GID (`instance_from_egid`, the kernel-enforced gate) and
  the wrapper name from its validated argv (`parse_invocation`). They travel
  as internal env vars (the `UNDERWRAP_*` pattern), stripped before the target
  exec, never propagated to the target. **No `ExecBundle` / wire change**, so
  the `exec.rs` "carries no target identity" invariant holds: we relay a
  *selector* resolved against the root-owned table, exactly as the target path
  is relayed today — never the resolved uid/gid.
- **The uid decouples from the binary.** `read.target` still says *what to
  exec*; the `(instance, wrapper)` selector says *which uid*. This is what lets
  the `Debug` variant of wrapper W run under **W's** uid (it brokers W's
  secret — same trust domain), eliminating the host-wide debug uid and the
  special debug-tool table row.

### Trust posture (unchanged from 2600)

The uid still comes **only** from the root-owned table; the worst a compromised
`underwrap-exec` can do is select a *different legitimate sandbox uid* (it can
never reach uid 0 or an arbitrary number — the floor + table membership
checks in `target_uid.rs` stand). That is the exact residual risk `2600`
already documented and accepted ("could at worst run a tool under another
tool's sandbox uid"). Re-keying does not enlarge it.

## Cutover (no parallel-with-legacy)

Land all three child tasks together as one change: the table format, the
module's user generation, and the `underwrap-ns` lookup move in lockstep.
There is no intermediate state where a path-keyed table and an
`(instance, wrapper)`-keyed reader coexist.

## Acceptance (story-level)

- Each configured `(instance, wrapper)` gets a distinct sandbox uid.
- Two wrappers using the **same** target binary (same instance) get
  **different** uids; the same binary across **two instances** gets
  **different** uids — proven in the VM test.
- `Debug` of wrapper W runs as W's sandbox uid; no `underwrap-target-*` uid is
  shared across wrappers or instances; no host-wide debug-tool uid exists.
- `ExecBundle` still carries no identity (round-trip test unchanged).
- Docs (`security.md`, `architecture.md`, `testing.md`) describe the
  `(instance, wrapper)` model; the "fully isolated instances" claim is now true
  at the uid layer.
- `cargo test --workspace`, `cargo clippy --all-targets --workspace -- -D
  warnings`, `cargo fmt --check`, and `nix build .#checks.x86_64-linux.vm-test`
  green.

## Out of scope

- **Per-invocation** uids (a fresh uid per run). That needs dynamic uid
  allocation, not declarative NixOS users; not pursued here.
- Removing or changing the Yama pin — it remains defense-in-depth.
- Any change to the caller-facing secret-hiding boundary (already complete in
  `2600`).
