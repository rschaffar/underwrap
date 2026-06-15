# Task: Document the runtime-dir model — including the *why*

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1010`, `1020`, `1030`

This work is a deliberate design decision with rejected alternatives, so the
docs must capture the **reasoning**, not just the mechanics.

## Where the rationale lives (the "why")

Primary home: **`docs/architecture.md`** — add a section
**"Runtime directory for isolated targets"**, alongside the existing
rationale-style sections (e.g. "Why the wrapper chain has two stages",
"Directory credential flow"). This is the canonical place for "why the system
is shaped this way."

That section must cover:

- The core principle: the target runs as a dedicated uid X, and a runtime dir is
  a per-uid resource, so **X gets X's own provisioned runtime dir** by default.
  This is the honest reflection of running as X — not sandboxing.
- Why X's runtime dir must be **provisioned by the system**: logind only creates
  `/run/user/<uid>` for login sessions, and `underwrap-ns` cannot create one at
  runtime (holds `cap_sys_admin,setpcap,setuid,setgid` but **not**
  `CAP_DAC_OVERRIDE`/`CAP_CHOWN`; `/run/user` is root-owned and logind's tree).
- Why the caller's live session runtime dir is **not** wired in at all: the
  `accessRuntimeDir` opt-in was designed and then removed (see `1030`).
  idmapping the caller's live `/run/user` is a category mismatch (it holds
  sockets + foreign fuse filesystems), leaks target scratch into the caller's
  session, depends on the caller having a session, and (with libsecret) re-exposes
  the caller's real session bus at `$XDG_RUNTIME_DIR/bus`. Not worth the surface
  until a real need appears; the escape-hatch design is recorded in `1030`.
- The `accessPaths` idmap is **best-effort per-submount**: recursive clone, idmap
  each mount individually, skip non-idmappable fuse submounts with a warning,
  keep idmappable ones (nested ZFS datasets, ext4/xfs/btrfs binds). The named
  dir's own filesystem must be idmappable (hard error otherwise). Document this.
- The connection to the threat model (`CLAUDE.md`): none of this exposes the
  brokered secret to the caller. Giving the target its own runtime dir is not
  "more restrictive than baseline" in any way that matters — it gets a working
  runtime dir; only the caller's *session* sockets are out of scope, by choice.
- **Rejected alternatives**, briefly, with why each was dropped:
  - idmap the caller's `/run/user` by default (category mismatch;
    submount-fragile via recursive `mount_setattr` EINVAL on fuse portals;
    scratch leak; needs a caller session; libsecret bus conflict) — and even as a
    default-off opt-in, removed entirely for now (`1030`);
  - a private per-invocation tmpfs (a namespace-local hack for "X has no runtime
    dir"; ephemeral; non-canonical — a provisioned per-uid dir is what a runtime
    dir actually is);
  - a shared `1777` base keyed on the caller uid (world-writable; cross-uid
    collisions; wrong ownership).

## Reference / mechanism docs to update

- **`docs/security.md`** — `XDG_RUNTIME_DIR` is always the target's own
  provisioned dir; **remove** the `accessRuntimeDir` paragraphs. Note the
  `accessPaths` best-effort per-submount idmap. Cross-link the architecture
  rationale section.
- **`docs/configuration.md`** — option table + the `XDG_RUNTIME_DIR` env-policy
  bullet: **remove** the `accessRuntimeDir` row; rename `xdgRuntimeDirBase` →
  `services.underwrap.targetRuntimeDirBase` (now a top-level option, default
  `/run/underwrap-runtime`, base for the *target* uid's provisioned dir); note
  the `accessPaths` best-effort submount behaviour.
- **`docs/wrapper-author-checklist.md`** — guidance: targets get their own
  runtime dir automatically (no flag); `accessPaths` for caller-owned dirs,
  noting submounts are best-effort (fuse skipped, ZFS/etc. kept). Remove
  `accessRuntimeDir`.
- **`examples/config.yaml`** — DONE: `agent-slack` needs no runtime-dir flag;
  `target_runtime_dir_base` renamed. (No `accessRuntimeDir` example — removed.)

## Acceptance

- `docs/architecture.md` has the rationale section with rejected alternatives.
- The four reference docs + example are consistent with the implemented
  behavior and cross-link the rationale.
- No doc still describes `accessRuntimeDir` as default-on or as the agent-slack
  mechanism.
