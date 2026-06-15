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
- Why the caller's session is a **separate, explicit opt-in** (`accessRuntimeDir`),
  not the default: idmapping the caller's live `/run/user` is a category
  mismatch (it holds sockets + foreign fuse filesystems), it leaks target
  scratch into the caller's session, and it depends on the caller having a
  session. idmap belongs to caller config/state dirs and to this opt-in.
- The connection to the threat model (`CLAUDE.md`): none of this exposes the
  brokered secret to the caller; the opt-in only grants the target more access
  to the caller's own resources, which is never a regression in the protected
  direction.
- **Rejected alternatives**, briefly, with why each was dropped:
  - idmap the caller's `/run/user` by default (category mismatch;
    submount-fragile via recursive `mount_setattr` EINVAL on fuse portals;
    scratch leak; needs a caller session);
  - a private per-invocation tmpfs (a namespace-local hack for "X has no runtime
    dir"; ephemeral; non-canonical — a provisioned per-uid dir is what a runtime
    dir actually is);
  - a shared `1777` base keyed on the caller uid (world-writable; cross-uid
    collisions; wrong ownership).

## Reference / mechanism docs to update

- **`docs/security.md`** — the "Caller-controlled environment is cleared"
  section's `XDG_RUNTIME_DIR` bullet (and any "Design invariants" /
  "Environment sanitization" mention): default = X's own provisioned dir;
  `accessRuntimeDir` opt-in = caller session, idmapped non-recursively; the
  libsecret-bus caveat scoped to the opt-in. Cross-link the architecture
  rationale section.
- **`docs/configuration.md`** — option table + the `XDG_RUNTIME_DIR` env-policy
  bullet: `accessRuntimeDir` default `false` and its real meaning; the runtime
  base knob's new meaning (base for the *target* uid's provisioned dir).
- **`docs/wrapper-author-checklist.md`** — guidance: targets get their own
  runtime dir; only set `accessRuntimeDir = true` if the tool needs the caller's
  session sockets (browser auth), noting the libsecret-bus consequence.
- **`examples/config.yaml`** — update the `agent-slack` example (it no longer
  needs any runtime-dir flag — the default works); show `accessRuntimeDir: true`
  on a browser-auth-style wrapper instead, with a one-line why.

## Acceptance

- `docs/architecture.md` has the rationale section with rejected alternatives.
- The four reference docs + example are consistent with the implemented
  behavior and cross-link the rationale.
- No doc still describes `accessRuntimeDir` as default-on or as the agent-slack
  mechanism.
